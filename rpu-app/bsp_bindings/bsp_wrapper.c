#include "bsp_wrapper.h"

int RpuIpiInitialize(XIpiPsu *InstancePtr, void *IntrHandler)
{
	XIpiPsu_Config *Config;
	int Status;
	const u32 PmcMask = 0x00000002U;

	Config = XIpiPsu_LookupConfig(XPAR_XIPIPSU_0_BASEADDR);
	if (Config == NULL) {
		return XST_FAILURE;
	}

	Status = XIpiPsu_CfgInitialize(
		InstancePtr,
		Config,
		Config->BaseAddress
	);
	if (Status != XST_SUCCESS) {
		return Status;
	}

	XIpiPsu_InterruptDisable(InstancePtr, PmcMask);
	XIpiPsu_ClearInterruptStatus(InstancePtr, PmcMask);

	Status = XSetupInterruptSystem(
		InstancePtr,
		IntrHandler,
		InstancePtr->Config.IntId,
		InstancePtr->Config.IntrParent,
		XINTERRUPT_DEFAULT_PRIORITY
	);
	if (Status != XST_SUCCESS) {
		return Status;
	}

	XIpiPsu_ClearInterruptStatus(InstancePtr, PmcMask);
	XIpiPsu_InterruptEnable(InstancePtr, PmcMask);

	return XST_SUCCESS;
}

u32 RpuIpiGetInterruptStatus(XIpiPsu *InstancePtr)
{
	return XIpiPsu_GetInterruptStatus(InstancePtr);
}

void RpuIpiClearInterruptStatus(XIpiPsu *InstancePtr, u32 Mask)
{
	XIpiPsu_ClearInterruptStatus(InstancePtr, Mask);
}
