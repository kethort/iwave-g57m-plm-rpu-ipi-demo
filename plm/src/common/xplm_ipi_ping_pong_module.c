#include "xplm_ipi_ping_pong_module.h"

#include "xplmi_cmd.h"
#include "xplmi_debug.h"
#include "xplmi_ipi.h"
#include "xplmi_modules.h"
#include "xstatus.h"

#define XPLM_IPI_USER_MODULE_INDEX	(0U)
#define XPLM_IPI_API_PING		(1U)
#define XPLM_IPI_REQUEST_WORDS		(3U)
#define XPLM_IPI_REQUEST_MAGIC		(0x52505531U)
#define XPLM_IPI_RESPONSE_XOR		(0xA5A55A5AU)
#define XPLM_IPI_ROTL32(Value, Shift) \
	(((Value) << (Shift)) | ((Value) >> (32U - (Shift))))

static u32 PlmReceiveCount;

static int XPlm_IpiPingCommandHandler(XPlmi_Cmd *Cmd)
{
	u32 ReceivedValue;
	u32 Sequence;
	u32 RequestToken;
	u32 ExpectedToken;
	int Status;

	ReceivedValue = Cmd->Payload[0U];
	Sequence = Cmd->Payload[1U];
	RequestToken = Cmd->Payload[2U];
	ExpectedToken = XPLM_IPI_REQUEST_MAGIC ^
		XPLM_IPI_ROTL32(ReceivedValue, 7U) ^
		XPLM_IPI_ROTL32(Sequence, 19U);
	PlmReceiveCount++;

	if ((Cmd->Len != XPLM_IPI_REQUEST_WORDS) ||
	    (RequestToken != ExpectedToken)) {
		XPlmi_Printf(
			DEBUG_PRINT_ALWAYS,
			"PLM IPI PING-PONG: INVALID request #%lu len=%lu "
			"counter=%lu sequence=%lu token=0x%08lx expected=0x%08lx\r\n",
			(unsigned long)PlmReceiveCount,
			(unsigned long)Cmd->Len,
			(unsigned long)ReceivedValue,
			(unsigned long)Sequence,
			(unsigned long)RequestToken,
			(unsigned long)ExpectedToken
		);

		Cmd->Response[0U] = (u32)XST_FAILURE;
		Cmd->Response[1U] = ReceivedValue;
		Cmd->Response[2U] = Sequence;
		Cmd->Response[3U] = 0U;
	} else {
		Cmd->Response[0U] = (u32)XST_SUCCESS;
		Cmd->Response[1U] = ReceivedValue + 1U;
		Cmd->Response[2U] = Sequence;
		Cmd->Response[3U] = RequestToken ^ XPLM_IPI_RESPONSE_XOR;
	}

	XPlmi_Printf(
		DEBUG_PRINT_ALWAYS,
		"PLM IPI PING-PONG: received request #%lu counter=%lu "
		"sequence=%lu token=0x%08lx\r\n",
		(unsigned long)PlmReceiveCount,
		(unsigned long)ReceivedValue,
		(unsigned long)Sequence,
		(unsigned long)RequestToken
	);

	/*
	 * The normal PLMI command path writes and acknowledges a response but does
	 * not generate a reverse IPI. Handle the response here so the RPU's IPI
	 * interrupt fires only after the response buffer is ready.
	 */
	Cmd->AckInPLM = (u8)FALSE;
	XPlmi_SendResponseandAck(Cmd->IpiMask, Cmd->Response);

	XPlmi_Printf(
		DEBUG_PRINT_ALWAYS,
		"PLM IPI PING-PONG: response status=0x%08lx value=%lu "
		"sequence=%lu token=0x%08lx, triggering RPU\r\n",
		(unsigned long)Cmd->Response[0U],
		(unsigned long)Cmd->Response[1U],
		(unsigned long)Cmd->Response[2U],
		(unsigned long)Cmd->Response[3U]
	);

	/*
	 * This trigger transfers UART ownership back to the RPU. Do not print
	 * afterward unless the trigger failed and the RPU therefore cannot run.
	 */
	Status = XPlmi_IpiTrigger(Cmd->IpiMask);
	if (Status != XST_SUCCESS) {
		XPlmi_Printf(
			DEBUG_PRINT_ALWAYS,
			"PLM IPI PING-PONG: RPU trigger failed: 0x%08lx\r\n",
			(unsigned long)Status
		);
	}

	return Status;
}

static XPlmi_ModuleCmd IpiPingPongCommands[] = {
	XPLMI_MODULE_COMMAND(NULL),
	XPLMI_MODULE_COMMAND(XPlm_IpiPingCommandHandler),
};

static XPlmi_AccessPerm_t IpiPingPongAccess[] = {
	XPLMI_ALL_IPI_NO_ACCESS(0U),
	XPLMI_ALL_IPI_FULL_ACCESS(XPLM_IPI_API_PING),
};

static XPlmi_Module IpiPingPongModule = {
	.Id = 0U,
	.CmdAry = IpiPingPongCommands,
	.CmdCnt = XPLMI_ARRAY_SIZE(IpiPingPongCommands),
	.AccessPermBufferPtr = IpiPingPongAccess,
};

int XPlm_IpiPingPongModuleInit(void)
{
	XPlmi_Printf(
		DEBUG_PRINT_ALWAYS,
		"PLM IPI PING-PONG: module initialization started\r\n"
	);

	PlmReceiveCount = 0U;

	IpiPingPongModule.Id =
		XPLMI_SET_USER_MODULE_ID(XPLM_IPI_USER_MODULE_INDEX);

	XPlmi_ModuleRegister(&IpiPingPongModule);

	XPlmi_Printf(
		DEBUG_PRINT_ALWAYS,
		"PLM IPI PING-PONG: registered module ID=0x%02lx, API=%lu\r\n",
		(unsigned long)IpiPingPongModule.Id,
		(unsigned long)XPLM_IPI_API_PING
	);

	return XST_SUCCESS;
}
