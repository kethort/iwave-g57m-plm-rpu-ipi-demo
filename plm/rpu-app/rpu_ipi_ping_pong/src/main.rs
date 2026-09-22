#![no_std]
#![no_main]

#[cfg(feature = "remoteproc")]
mod remoteproc;

use bsp_bindings::{
    sleep, xil_printf, RpuIpiClearInterruptStatus, RpuIpiGetInterruptStatus, RpuIpiInitialize,
    XIpiPsu, XIpiPsu_ReadMessage, XIpiPsu_TriggerIpi, XIpiPsu_WriteMessage, XIPIPSU_BUF_TYPE_MSG,
    XIPIPSU_BUF_TYPE_RESP, XST_FAILURE, XST_SUCCESS,
};
use core::ffi::{c_int, c_ulong, c_void};
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

const PMC_MASK: u32 = 0x0000_0002;
const REQUEST_PAYLOAD_WORDS: u32 = 3;
const USER_COMMAND_HEADER: u32 = (REQUEST_PAYLOAD_WORDS << 16) | (0x80 << 8) | 1;
const REQUEST_WORDS: u32 = REQUEST_PAYLOAD_WORDS + 1;
const RESPONSE_WORDS: u32 = 4;
const REQUEST_MAGIC: u32 = 0x5250_5531;
const RESPONSE_XOR: u32 = 0xA5A5_5A5A;
const TOTAL_PING_PONGS: u32 = 100;
const RESPONSE_WAIT_ITERATIONS: u32 = 100_000_000;
const PLM_BOOT_SETTLE_SECONDS: u32 = 5;

static mut IPI: MaybeUninit<XIpiPsu> = MaybeUninit::uninit();
static RESPONSE_READY: AtomicBool = AtomicBool::new(false);
static RESPONSE_VALUE: AtomicU32 = AtomicU32::new(0);
static RESPONSE_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static RESPONSE_TOKEN: AtomicU32 = AtomicU32::new(0);
static RESPONSE_STATUS: AtomicI32 = AtomicI32::new(XST_FAILURE as i32);
static IPI_INTERRUPT_COUNT: AtomicU32 = AtomicU32::new(0);

fn ipi_ptr() -> *mut XIpiPsu {
    core::ptr::addr_of_mut!(IPI).cast::<XIpiPsu>()
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn RpuIpiInterruptHandler(callback: *mut c_void) {
    let ipi = callback.cast::<XIpiPsu>();
    let interrupt_status = RpuIpiGetInterruptStatus(ipi);
    if interrupt_status & PMC_MASK == 0 {
        return;
    }

    let mut response = [0_u32; RESPONSE_WORDS as usize];
    let status = XIpiPsu_ReadMessage(
        ipi,
        PMC_MASK,
        response.as_mut_ptr(),
        RESPONSE_WORDS,
        XIPIPSU_BUF_TYPE_RESP as u8,
    );

    RpuIpiClearInterruptStatus(ipi, PMC_MASK);
    if status == XST_SUCCESS as i32 {
        RESPONSE_STATUS.store(response[0] as i32, Ordering::Relaxed);
        RESPONSE_VALUE.store(response[1], Ordering::Relaxed);
        RESPONSE_SEQUENCE.store(response[2], Ordering::Relaxed);
        RESPONSE_TOKEN.store(response[3], Ordering::Relaxed);
    } else {
        RESPONSE_STATUS.store(status, Ordering::Relaxed);
        RESPONSE_VALUE.store(0, Ordering::Relaxed);
        RESPONSE_SEQUENCE.store(0, Ordering::Relaxed);
        RESPONSE_TOKEN.store(0, Ordering::Relaxed);
    }
    IPI_INTERRUPT_COUNT.fetch_add(1, Ordering::Relaxed);
    RESPONSE_READY.store(true, Ordering::Release);
}

unsafe fn send_user_command(counter: u32, sequence: u32, reply: &mut u32) -> c_int {
    let request_token = REQUEST_MAGIC ^ counter.rotate_left(7) ^ sequence.rotate_left(19);
    let expected_response_token = request_token ^ RESPONSE_XOR;
    let request = [USER_COMMAND_HEADER, counter, sequence, request_token];
    RESPONSE_READY.store(false, Ordering::Relaxed);
    RESPONSE_VALUE.store(0, Ordering::Relaxed);
    RESPONSE_SEQUENCE.store(0, Ordering::Relaxed);
    RESPONSE_TOKEN.store(0, Ordering::Relaxed);
    RESPONSE_STATUS.store(XST_FAILURE as i32, Ordering::Relaxed);

    let mut status = XIpiPsu_WriteMessage(
        ipi_ptr(),
        PMC_MASK,
        request.as_ptr(),
        REQUEST_WORDS,
        XIPIPSU_BUF_TYPE_MSG as u8,
    );
    if status != XST_SUCCESS as i32 {
        return status;
    }

    xil_printf(
        b"RPU (Rust): TX counter=%lu sequence=%lu token=0x%08lx\r\n\0"
            .as_ptr()
            .cast(),
        counter as c_ulong,
        sequence as c_ulong,
        request_token as c_ulong,
    );

    status = XIpiPsu_TriggerIpi(ipi_ptr(), PMC_MASK);
    if status != XST_SUCCESS as i32 {
        return status;
    }

    let mut wait_count = 0;
    while wait_count < RESPONSE_WAIT_ITERATIONS && !RESPONSE_READY.load(Ordering::Acquire) {
        core::hint::spin_loop();
        wait_count += 1;
    }

    if !RESPONSE_READY.load(Ordering::Acquire) {
        xil_printf(
            b"RPU (Rust): timeout waiting for PLM response interrupt\r\n\0"
                .as_ptr()
                .cast(),
        );
        return XST_FAILURE as c_int;
    }

    let response_status = RESPONSE_STATUS.load(Ordering::Relaxed);
    let response_value = RESPONSE_VALUE.load(Ordering::Relaxed);
    let response_sequence = RESPONSE_SEQUENCE.load(Ordering::Relaxed);
    let response_token = RESPONSE_TOKEN.load(Ordering::Relaxed);
    xil_printf(
        b"RPU (Rust): RX ISR #%lu status=0x%08lx value=%lu sequence=%lu token=0x%08lx\r\n\0"
            .as_ptr()
            .cast(),
        IPI_INTERRUPT_COUNT.load(Ordering::Relaxed) as c_ulong,
        response_status as u32 as c_ulong,
        response_value as c_ulong,
        response_sequence as c_ulong,
        response_token as c_ulong,
    );

    if response_status != XST_SUCCESS as i32 {
        return response_status;
    }
    if response_value != counter + 1
        || response_sequence != sequence
        || response_token != expected_response_token
    {
        xil_printf(
            b"RPU (Rust): INVALID response expected value=%lu sequence=%lu token=0x%08lx\r\n\0"
                .as_ptr()
                .cast(),
            (counter + 1) as c_ulong,
            sequence as c_ulong,
            expected_response_token as c_ulong,
        );
        return XST_FAILURE as c_int;
    }
    *reply = response_value;
    XST_SUCCESS as c_int
}

#[no_mangle]
pub unsafe extern "C" fn main() -> c_int {
    xil_printf(
        b"\r\nRPU (Rust): PLM IPI demo starting\r\n\0"
            .as_ptr()
            .cast(),
    );

    #[cfg(feature = "remoteproc")]
    // Keep the resource table reachable so link-time GC retains the section.
    core::hint::black_box(remoteproc::resource_table_address());

    let status = RpuIpiInitialize(
        ipi_ptr(),
        RpuIpiInterruptHandler as *const () as *mut c_void,
    );
    if status != XST_SUCCESS as i32 {
        xil_printf(
            b"RPU (Rust): IPI initialization failed: 0x%08lx\r\n\0"
                .as_ptr()
                .cast(),
            status as u32 as c_ulong,
        );
        return status;
    }

    xil_printf(
        b"RPU (Rust): waiting %lu seconds for PLM command services\r\n\0"
            .as_ptr()
            .cast(),
        PLM_BOOT_SETTLE_SECONDS as c_ulong,
    );
    sleep(PLM_BOOT_SETTLE_SECONDS);

    let mut counter = 1;
    for ping_pong in 1..=TOTAL_PING_PONGS {
        let mut reply = 0;
        let status = send_user_command(counter, ping_pong, &mut reply);
        if status != XST_SUCCESS as i32 {
            xil_printf(
                b"RPU (Rust): ping-pong #%lu failed: 0x%08lx\r\n\0"
                    .as_ptr()
                    .cast(),
                ping_pong as c_ulong,
                status as u32 as c_ulong,
            );
            return status;
        }

        if reply != counter + 1 {
            xil_printf(
                b"RPU (Rust): ping-pong #%lu invalid value=%lu expected=%lu\r\n\0"
                    .as_ptr()
                    .cast(),
                ping_pong as c_ulong,
                reply as c_ulong,
                (counter + 1) as c_ulong,
            );
            return XST_FAILURE as c_int;
        }

        xil_printf(
            b"RPU (Rust): ping-pong #%lu complete, reply=%lu\r\n\0"
                .as_ptr()
                .cast(),
            ping_pong as c_ulong,
            reply as c_ulong,
        );
        counter = reply;
        sleep(1);
    }

    xil_printf(
        b"RPU (Rust): completed %lu PLM IPI ping-pongs, final value=%lu\r\n\0"
            .as_ptr()
            .cast(),
        TOTAL_PING_PONGS as c_ulong,
        counter as c_ulong,
    );

    loop {
        sleep(1);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
