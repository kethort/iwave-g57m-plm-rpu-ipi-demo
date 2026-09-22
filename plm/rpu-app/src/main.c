#include "sleep.h"
#include "xinterrupt_wrap.h"
#include "xil_printf.h"
#include "xipipsu.h"
#include "xparameters.h"
#include "xstatus.h"

#define PMC_MASK                 (0x00000002U)

#define USER_MODULE_ID           (0x80U)
#define USER_RPU_API_ID          (1U)
#define USER_COMMAND_HEADER      ((USER_MODULE_ID << 8U) | USER_RPU_API_ID)

#define REQUEST_WORDS            (2U)
#define RESPONSE_WORDS           (2U)
#define TOTAL_PING_PONGS         (100U)
#define RESPONSE_WAIT_ITERATIONS (100000000U)
#define PLM_BOOT_SETTLE_SECONDS  (5U)

#ifndef XPAR_XIPIPSU_0_BASEADDR
#error "No XIpiPsu instance was generated. Confirm R5-0 is master of IPI1 in CIPS and regenerate the platform."
#endif

static XIpiPsu Ipi;
static volatile u32 ResponseReady;
static volatile u32 ResponseValue;
static volatile int ResponseStatus;
static volatile u32 IpiInterruptCount;

void __attribute__((noinline)) RpuIpiInterruptHandler(void *CallbackRef)
{
	XIpiPsu *IpiInstance = (XIpiPsu *)CallbackRef;
	u32 InterruptStatus;
	u32 Response[RESPONSE_WORDS] = {0U, 0U};
	int Status;

	InterruptStatus = XIpiPsu_GetInterruptStatus(IpiInstance);
	if ((InterruptStatus & PMC_MASK) == 0U) {
		return;
	}

	Status = XIpiPsu_ReadMessage(
		IpiInstance,
		PMC_MASK,
		Response,
		RESPONSE_WORDS,
		XIPIPSU_BUF_TYPE_RESP
	);

	/*
	 * Clearing the destination status acknowledges the PLM's IPI and clears
	 * the corresponding observation bit at the PLM.
	 */
	XIpiPsu_ClearInterruptStatus(IpiInstance, PMC_MASK);

	if (Status == XST_SUCCESS) {
		ResponseStatus = (int)Response[0];
		ResponseValue = Response[1];
	} else {
		ResponseStatus = Status;
		ResponseValue = 0U;
	}

	IpiInterruptCount++;
	ResponseReady = 1U;
}

static int IpiInit(void)
{
	XIpiPsu_Config *Config;
	int Status;

	Config = XIpiPsu_LookupConfig(XPAR_XIPIPSU_0_BASEADDR);
	if (Config == NULL) {
		return XST_FAILURE;
	}

	Status = XIpiPsu_CfgInitialize(
		&Ipi,
		Config,
		Config->BaseAddress
	);

	if (Status != XST_SUCCESS) {
		return Status;
	}

	XIpiPsu_InterruptDisable(&Ipi, PMC_MASK);
	XIpiPsu_ClearInterruptStatus(&Ipi, PMC_MASK);

	Status = XSetupInterruptSystem(
		&Ipi,
		(void *)&RpuIpiInterruptHandler,
		Ipi.Config.IntId,
		Ipi.Config.IntrParent,
		XINTERRUPT_DEFAULT_PRIORITY
	);
	if (Status != XST_SUCCESS) {
		return Status;
	}

	XIpiPsu_ClearInterruptStatus(&Ipi, PMC_MASK);
	XIpiPsu_InterruptEnable(&Ipi, PMC_MASK);

	return XST_SUCCESS;
}

static int SendUserCommand(u32 Counter, u32 *Reply)
{
	u32 Request[REQUEST_WORDS];
	u32 WaitCount;
	int Status;

	if (Reply == NULL) {
		return XST_INVALID_PARAM;
	}

	Request[0] = USER_COMMAND_HEADER;
	Request[1] = Counter;

	ResponseReady = 0U;
	ResponseValue = 0U;
	ResponseStatus = XST_FAILURE;

	Status = XIpiPsu_WriteMessage(
		&Ipi,
		PMC_MASK,
		Request,
		REQUEST_WORDS,
		XIPIPSU_BUF_TYPE_MSG
	);
	if (Status != XST_SUCCESS) {
		return Status;
	}

	/*
	 * Print before triggering PLM. The IPI transfers UART ownership to PLM
	 * until its response interrupt returns ownership to this RPU.
	 */
	xil_printf(
		"RPU: triggering PLM, counter=%lu\r\n",
		(unsigned long)Counter
	);

	Status = XIpiPsu_TriggerIpi(&Ipi, PMC_MASK);
	if (Status != XST_SUCCESS) {
		return Status;
	}

	for (WaitCount = 0U;
	     (WaitCount < RESPONSE_WAIT_ITERATIONS) && (ResponseReady == 0U);
	     WaitCount++) {
		/* The IPI ISR sets ResponseReady after reading the PLM response. */
	}

	if (ResponseReady == 0U) {
		xil_printf(
			"RPU: timeout waiting for PLM response interrupt\r\n"
		);
		return XST_FAILURE;
	}

	xil_printf(
		"RPU: ISR #%lu response status=0x%08lx value=%lu\r\n",
		(unsigned long)IpiInterruptCount,
		(unsigned long)ResponseStatus,
		(unsigned long)ResponseValue
	);

	if (ResponseStatus != XST_SUCCESS) {
		return ResponseStatus;
	}

	*Reply = ResponseValue;

	return XST_SUCCESS;
}

int main(void)
{
	u32 Counter = 1U;
	u32 Reply = 0U;
	u32 PingPong;
	int Status;

	xil_printf("\r\nRPU: PLM user-module IPI demo starting\r\n");

	Status = IpiInit();
	if (Status != XST_SUCCESS) {
		xil_printf(
			"RPU: IPI initialization failed: 0x%08lx\r\n",
			(unsigned long)Status
		);
		return Status;
	}

	IpiInterruptCount = 0U;

	/*
	 * PLM hands off the RPU image before its own boot tasks have all finished.
	 * Keep the RPU as initiator, but let PLM reach its command loop first.
	 */
	xil_printf(
		"RPU: waiting %lu seconds for PLM command services\r\n",
		(unsigned long)PLM_BOOT_SETTLE_SECONDS
	);
	sleep(PLM_BOOT_SETTLE_SECONDS);

	for (PingPong = 1U; PingPong <= TOTAL_PING_PONGS; PingPong++) {
		Status = SendUserCommand(Counter, &Reply);
		if (Status != XST_SUCCESS) {
			xil_printf(
				"RPU: ping-pong #%lu failed: 0x%08lx\r\n",
				(unsigned long)PingPong,
				(unsigned long)Status
			);

			return Status;
		}

		if (Reply != (Counter + 1U)) {
			xil_printf(
				"RPU: ping-pong #%lu returned invalid value=%lu, "
				"expected=%lu\r\n",
				(unsigned long)PingPong,
				(unsigned long)Reply,
				(unsigned long)(Counter + 1U)
			);

			return XST_FAILURE;
		}

		xil_printf(
			"RPU: ping-pong #%lu complete, reply=%lu\r\n",
			(unsigned long)PingPong,
			(unsigned long)Reply
		);

		Counter = Reply;
		sleep(1U);
	}

	xil_printf(
		"RPU: completed %lu PLM IPI ping-pongs, final value=%lu\r\n",
		(unsigned long)TOTAL_PING_PONGS,
		(unsigned long)Counter
	);

	/* Keep the RPU application available for attach-to-running debugging. */
	for (;;) {
		sleep(1U);
	}
}
