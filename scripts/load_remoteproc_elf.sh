#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Keep the current defaults unchanged.
BOARD_USER="${BOARD_USER:-root}"
BOARD_PASSWORD="${BOARD_PASSWORD:-root}"
BOARD_IP="${BOARD_IP:-192.168.1.20}"

REMOTEPROC="${REMOTEPROC:-/sys/class/remoteproc/remoteproc0}"
DEFAULT_ELF="${DEFAULT_ELF:-${PROJECT_ROOT}/plm/rpu-app/target/armv7r-none-eabihf/debug/rpu_ipi_ping_pong}"

# Debug helpers for Vitis / XSDB.
BREAK_ADDR="${BREAK_ADDR:-0x4112c}"
HW_SERVER_URL="${HW_SERVER_URL:-TCP:127.0.0.1:3121}"
HW_SERVER_LOG="${HW_SERVER_LOG:-/tmp/hw_server_remoteproc.log}"

KNOWN_HOSTS="${KNOWN_HOSTS:-${HOME}/.ssh/known_hosts}"

die() {
	echo "ERROR: $*" >&2
	exit 1
}

require_command() {
	local command_name="$1"

	command -v "${command_name}" >/dev/null 2>&1 ||
		die "Required command not found: ${command_name}"
}

resolve_break_addr() {
	local nm_tool=""
	local resolved_addr=""

	if command -v armr5-none-eabi-nm >/dev/null 2>&1; then
		nm_tool="armr5-none-eabi-nm"
	elif command -v nm >/dev/null 2>&1; then
		nm_tool="nm"
	fi

	if [[ -n "${nm_tool}" ]]; then
		resolved_addr="$(${nm_tool} -n "${LOCAL_ELF}" | awk '$3 == "main" && $2 ~ /^[Tt]$/ { printf "0x%s\n", $1; exit }')"
	fi

	if [[ -n "${resolved_addr}" ]]; then
		BREAK_ADDR="${resolved_addr}"
		echo "Resolved main breakpoint from ELF: ${BREAK_ADDR}"
	else
		echo "Using configured breakpoint address: ${BREAK_ADDR}"
	fi
}

run_with_askpass() {
	local askpass_script
	local status

	askpass_script="$(mktemp)"

	cat > "${askpass_script}" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${ASKPASS_PASSWORD}"
EOF
	chmod 700 "${askpass_script}"

	env \
		ASKPASS_PASSWORD="${BOARD_PASSWORD}" \
		SSH_ASKPASS="${askpass_script}" \
		SSH_ASKPASS_REQUIRE=force \
		DISPLAY="${DISPLAY:-:0}" \
		setsid -w "$@"
	status=$?

	rm -f "${askpass_script}"

	return "${status}"
}

refresh_known_hosts() {
	mkdir -p "${HOME}/.ssh"
	touch "${KNOWN_HOSTS}"

	chmod 0700 "${HOME}/.ssh"
	chmod 0600 "${KNOWN_HOSTS}"

	echo "Refreshing SSH host key for ${BOARD_IP}..."

	ssh-keygen -f "${KNOWN_HOSTS}" -R "${BOARD_IP}" >/dev/null 2>&1 || true
	ssh-keygen -f "${KNOWN_HOSTS}" -R "[${BOARD_IP}]:22" >/dev/null 2>&1 || true

	echo "Scanning SSH host key..."

	ssh-keyscan -H -T 5 "${BOARD_IP}" >> "${KNOWN_HOSTS}" 2>/dev/null ||
		die "Failed to obtain SSH host key from ${BOARD_IP}"
}

run_remote_sudo() {
	local remote_command="$1"

	run_with_askpass ssh \
		-T \
		-o PreferredAuthentications=password \
		-o PubkeyAuthentication=no \
		-o NumberOfPasswordPrompts=1 \
		-o StrictHostKeyChecking=yes \
		-o UserKnownHostsFile="${KNOWN_HOSTS}" \
		"${BOARD_USER}@${BOARD_IP}" \
		"sudo -S -p '' sh -c $(printf '%q' "${remote_command}")" \
		<<< "${BOARD_PASSWORD}"
}

hw_server_host() {
	local address="${HW_SERVER_URL#TCP:}"
	printf '%s\n' "${address%:*}"
}

hw_server_port() {
	local address="${HW_SERVER_URL#TCP:}"
	printf '%s\n' "${address##*:}"
}

hw_server_is_ready() {
	local host
	local port

	host="$(hw_server_host)"
	port="$(hw_server_port)"

	timeout 1 bash -c "</dev/tcp/${host}/${port}" >/dev/null 2>&1
}

ensure_hw_server() {
	local hw_server_pid
	local attempt

	if hw_server_is_ready; then
		echo "hw_server is already available at ${HW_SERVER_URL}"
		return 0
	fi

	echo
	echo "Starting hw_server..."
	echo "Log: ${HW_SERVER_LOG}"

	: > "${HW_SERVER_LOG}"

	nohup hw_server > "${HW_SERVER_LOG}" 2>&1 &
	hw_server_pid=$!

	for attempt in $(seq 1 40); do
		if hw_server_is_ready; then
			echo "hw_server is ready at ${HW_SERVER_URL}"
			return 0
		fi

		if ! kill -0 "${hw_server_pid}" 2>/dev/null; then
			cat "${HW_SERVER_LOG}" >&2 || true
			die "hw_server exited before becoming ready"
		fi

		sleep 0.25
	done

	cat "${HW_SERVER_LOG}" >&2 || true
	die "Timed out waiting for hw_server at ${HW_SERVER_URL}"
}

copy_firmware() {
	local local_elf="$1"
	local firmware_name="$2"
	local remote_tmp="/tmp/${firmware_name}"

	echo
	echo "Copying ELF to the board..."

	run_with_askpass scp \
		-o PreferredAuthentications=password \
		-o PubkeyAuthentication=no \
		-o NumberOfPasswordPrompts=1 \
		-o StrictHostKeyChecking=yes \
		-o UserKnownHostsFile="${KNOWN_HOSTS}" \
		"${local_elf}" \
		"${BOARD_USER}@${BOARD_IP}:${remote_tmp}"
}

stop_and_install_firmware() {
	local firmware_name="$1"
	local remote_tmp="/tmp/${firmware_name}"
	local remote_command

	echo
	echo "Stopping remoteproc and installing firmware..."

	printf -v remote_command '%s\n' \
		"set -e" \
		"test -e '${REMOTEPROC}/state'" \
		"test -e '${REMOTEPROC}/firmware'" \
		"echo stop > '${REMOTEPROC}/state' 2>/dev/null || true" \
		"mkdir -p /lib/firmware" \
		"cp '${remote_tmp}' '/lib/firmware/${firmware_name}'" \
		"chmod 0644 '/lib/firmware/${firmware_name}'" \
		"test -f '/lib/firmware/${firmware_name}'" \
		"echo 'Remoteproc state after stop:'" \
		"cat '${REMOTEPROC}/state'"

	run_remote_sudo "${remote_command}"
}

set_main_breakpoint() {
	local tcl_file
	local xsdb_status

	tcl_file="$(mktemp)"

	cat > "${tcl_file}" <<EOF
connect -url ${HW_SERVER_URL}

puts "Selecting Cortex-R5 #0..."
targets -set -nocase -filter {name =~ "*Cortex-R5 #0*"}

puts "Setting hardware breakpoint at Rust main: ${BREAK_ADDR}"
bpadd -type hw -addr ${BREAK_ADDR}

puts "Configured breakpoints:"
bplist
EOF

	echo
	echo "Configuring Cortex-R5 #0 main breakpoint..."

	set +e
	xsdb "${tcl_file}"
	xsdb_status=$?
	set -e

	rm -f "${tcl_file}"

	if [[ "${xsdb_status}" -ne 0 ]]; then
		die "Failed to configure Cortex-R5 #0 breakpoint"
	fi
}

start_remoteproc() {
	local firmware_name="$1"
	local remote_command

	echo
	echo "Starting firmware through remoteproc..."

	printf -v remote_command '%s\n' \
		"set -e" \
		"test -f '/lib/firmware/${firmware_name}'" \
		"echo '${firmware_name}' > '${REMOTEPROC}/firmware'" \
		"echo start > '${REMOTEPROC}/state'" \
		"echo" \
		"echo 'Remoteproc state:'" \
		"cat '${REMOTEPROC}/state'" \
		"echo" \
		"echo 'Remoteproc firmware:'" \
		"cat '${REMOTEPROC}/firmware'"

	run_remote_sudo "${remote_command}"
}

stop_remoteproc() {
	local remote_command

	echo
	echo "Stopping remoteproc..."

	printf -v remote_command '%s\n' \
		"set -e" \
		"echo stop > '${REMOTEPROC}/state' 2>/dev/null || true" \
		"echo" \
		"echo 'Remoteproc state:'" \
		"cat '${REMOTEPROC}/state' 2>/dev/null || true" \
		"echo" \
		"echo 'Remoteproc firmware:'" \
		"cat '${REMOTEPROC}/firmware' 2>/dev/null || true"

	run_remote_sudo "${remote_command}"
}

usage() {
	cat <<'USAGE'
Usage:
  ./load_remoteproc_elf.sh [options] [path/to/file.elf]
  ./load_remoteproc_elf.sh --stop [options]

Defaults:
  ELF file:      ./plm/rpu-app/target/armv7r-none-eabihf/debug/rpu_ipi_ping_pong
  Board user:    root
  Board password: root
  Board IP:      192.168.1.20
  Remoteproc:    /sys/class/remoteproc/remoteproc0
  HW server:     TCP:127.0.0.1:3121

Options:
  -u, --user USER          SSH user on the board
  -p, --password PASS      SSH password on the board
  -i, --ip IP              Board IP address
  -r, --remoteproc PATH    Remoteproc sysfs path
  -f, --firmware-name NAME Firmware filename to install under /lib/firmware
  -b, --break-addr ADDR    Hardware breakpoint address for XSDB
  -w, --hw-server URL      HW server URL for XSDB
  --no-breakpoint          Skip the XSDB breakpoint setup
  -s, --stop               Stop the current remoteproc firmware and exit
  -h, --help               Show this help

Examples:
  ./load_remoteproc_elf.sh
  ./load_remoteproc_elf.sh plm/rpu-app/target/armv7r-none-eabihf/debug/rpu_ipi_ping_pong
  ./load_remoteproc_elf.sh --user root --password root --ip 192.168.1.20 path/to/custom.elf
  ./load_remoteproc_elf.sh --stop
USAGE
}

require_command realpath
require_command ssh
require_command scp
require_command ssh-keygen
require_command ssh-keyscan
require_command hw_server
require_command xsdb
require_command timeout
require_command mktemp
require_command seq
require_command setsid

LOCAL_ELF=""
FIRMWARE_NAME=""
STOP_ONLY=0
NO_BREAKPOINT=0

while [[ $# -gt 0 ]]; do
	case "$1" in
		-h|--help)
			usage
			exit 0
			;;
		-u|--user)
			[[ $# -ge 2 ]] || die "--user needs a value"
			BOARD_USER="$2"
			shift 2
			;;
		-p|--password)
			[[ $# -ge 2 ]] || die "--password needs a value"
			BOARD_PASSWORD="$2"
			shift 2
			;;
		-i|--ip)
			[[ $# -ge 2 ]] || die "--ip needs a value"
			BOARD_IP="$2"
			shift 2
			;;
		-r|--remoteproc)
			[[ $# -ge 2 ]] || die "--remoteproc needs a value"
			REMOTEPROC="$2"
			shift 2
			;;
		-f|--firmware-name)
			[[ $# -ge 2 ]] || die "--firmware-name needs a value"
			FIRMWARE_NAME="$2"
			shift 2
			;;
		-b|--break-addr)
			[[ $# -ge 2 ]] || die "--break-addr needs a value"
			BREAK_ADDR="$2"
			shift 2
			;;
		-w|--hw-server)
			[[ $# -ge 2 ]] || die "--hw-server needs a value"
			HW_SERVER_URL="$2"
			shift 2
			;;
		--no-breakpoint)
			NO_BREAKPOINT=1
			shift
			;;
		-s|--stop)
			STOP_ONLY=1
			shift
			;;
		--)
			shift
			if [[ $# -gt 0 ]]; then
				LOCAL_ELF="$1"
				shift
			fi
			if [[ $# -gt 0 ]]; then
				die "Unexpected extra argument: $1"
			fi
			;;
		-*)
			die "Unknown option: $1"
			;;
		*)
			if [[ -n "${LOCAL_ELF}" ]]; then
				die "Unexpected extra argument: $1"
			fi
			LOCAL_ELF="$1"
			shift
			;;
	esac
done

if [[ "${STOP_ONLY}" -eq 1 ]]; then
	if [[ -n "${LOCAL_ELF}" ]]; then
		die "--stop does not take an ELF path"
	fi
	refresh_known_hosts
	stop_remoteproc
	exit 0
fi

if [[ -z "${LOCAL_ELF}" ]]; then
	LOCAL_ELF="${DEFAULT_ELF}"
fi

[[ -f "${LOCAL_ELF}" ]] || die "ELF not found: ${LOCAL_ELF}"

LOCAL_ELF="$(realpath "${LOCAL_ELF}")"

resolve_break_addr

if [[ -z "${FIRMWARE_NAME}" ]]; then
	FIRMWARE_NAME="$(basename "${LOCAL_ELF}")"
else
	FIRMWARE_NAME="$(basename "${FIRMWARE_NAME}")"
fi

echo "Board:               ${BOARD_USER}@${BOARD_IP}"
echo "Local ELF:           ${LOCAL_ELF}"
echo "Firmware name:       ${FIRMWARE_NAME}"
echo "Remoteproc instance: ${REMOTEPROC}"
echo "HW server:           ${HW_SERVER_URL}"
echo "Main breakpoint:     ${BREAK_ADDR}"

refresh_known_hosts

if [[ "${NO_BREAKPOINT}" -eq 0 ]]; then
	ensure_hw_server
	set_main_breakpoint
else
	echo
	echo "Skipping breakpoint setup"
fi

copy_firmware "${LOCAL_ELF}" "${FIRMWARE_NAME}"
stop_and_install_firmware "${FIRMWARE_NAME}"
start_remoteproc "${FIRMWARE_NAME}"
