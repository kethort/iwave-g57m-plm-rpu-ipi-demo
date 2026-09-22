# iWave G57M PLM/RPU IPI Demo

This repository contains the maintained source for a Versal PLM user-module
experiment and a matching Cortex-R5 firmware application.

The experiment builds:

- a custom PLM with an `xilplmi` user module registered at runtime;
- an RPU bare-metal application that sends IPI commands to the PLM;
- an optional production-style PDI containing both the custom PLM and the RPU
  ELF.

Generated Vitis workspaces, BSP output, ELFs, PDIs, logs, and board-specific XSA
files are intentionally not tracked here.

## Source Layout

```text
plm/
├── build-plm
└── src/
    └── common/
        ├── xplm_ipi_ping_pong_module.c
        └── xplm_ipi_ping_pong_module.h
rpu-app/
├── Cargo.toml
├── bsp_bindings/
└── rpu_ipi_ping_pong/
    └── src/
        ├── main.rs
        └── remoteproc.rs
```

`plm/build-plm` is a Vitis Python script. Run it through `vitis -s`; do not run
it with the host Python interpreter.

## Prerequisites

- AMD Vitis 2025.2 available as `vitis`, or an equivalent absolute launcher path.
- `bootgen` from the same Vitis installation on `PATH`.
- A Versal XSA exported with a device image included.
- R5-0 and PMC IPI connectivity enabled in the hardware design. The RPU source
  expects an `XIpiPsu` instance and uses IPI1-style PMC communication.

## Build

From the repository root:

```bash
vitis -s ./plm/build-plm \
  ./build \
  --xsa ./system.xsa \
  --custom-source-dir ./plm/src \
  --register-module xplm_ipi_ping_pong_module.h:XPlm_IpiPingPongModuleInit \
  --user-modules-count 1 \
  --rpu-source ./rpu-app \
  --rpu-app-name rpu_ipi_ping_pong \
  --rpu-cargo-package rpu_ipi_ping_pong \
  --rpu-platform-name rpu_platform \
  --rpu-processor psv_cortexr5_0 \
  --rpu-domain standalone_psv_cortexr5_0 \
  --rpu-core r5-0 \
  --symlink-sources \
  --force-regenerate \
  --embed-rpu-in-pdi
```

The script creates or regenerates a PLM platform/application, overlays the
custom PLM source, patches `XPlm_ModuleInit()` to call the user module init
function, builds `plm.elf`, creates an RPU Vitis application, exposes the Rust
workspace to the generated component, builds the RPU ELF with Cargo, and emits
PDI artifacts.

Typical outputs:

```text
build/plm/build/plm.elf
build/rpu_ipi_ping_pong/build/rpu_ipi_ping_pong.elf
build/PLM_CUSTOM_JTAG.pdi
build/PLM_RPU_PRODUCTION.pdi
```

`PLM_CUSTOM_JTAG.pdi` is the PLM-only debug launch image. When
`--embed-rpu-in-pdi` is used, `PLM_RPU_PRODUCTION.pdi` packages the PLM and RPU
firmware together so the RPU starts from the boot image.

## Vitis 2025.2 User-Module Workaround

Enabling `XILPLMI_user_modules_count` in Vitis 2025.2 can generate xilplmi BSP
headers in an order where `xplmi_cmd.h` references
`XPLMI_USER_MODULE_START_INDEX` before the value is visible. The resulting
platform build fails before the PLM application can be compiled.

`plm/build-plm` handles this in the platform build path:

1. It sets `XILPLMI_user_modules_count`.
2. It asks Vitis to generate platform sources when that API is available.
3. It patches generated xilplmi header locations to define or guard
   `XPLMI_USER_MODULE_START_INDEX`.
4. If the first platform build still fails, it applies the same patch again and
   retries once.

The workaround is limited to generated BSP output under the Vitis workspace. The
tracked PLM source remains a normal user module.

## Runtime Behavior

The PLM user module registers a command under user module ID `0x80`, API `1`.
The Rust RPU firmware sends a counter value over IPI, waits for the PLM response
interrupt, checks that the reply increments the counter, and repeats the
exchange.

When built through `plm/build-plm`, the Rust firmware is compiled with the
`remoteproc` feature. That keeps a minimal `.resource_table` section in the ELF
so Linux remoteproc can recognize the image even though this demo does not use
RPMsg vrings or carveouts.

The PLM response path explicitly sends the PLMI response and triggers the RPU
after the response buffer is ready. This keeps the RPU interrupt-driven instead
of relying on polling.

PLM and RPU may share a UART. Keep diagnostic output short because the processors
can interleave characters while the demo hands control back and forth.

## Clean Rebuild

Remove generated output, then rerun the build:

```bash
rm -rf build .Xil
```

Do not delete the tracked `plm/` source tree.
