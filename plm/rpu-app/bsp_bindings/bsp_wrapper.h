#ifndef RPU_BSP_WRAPPER_H
#define RPU_BSP_WRAPPER_H

#include "sleep.h"
#include "xil_printf.h"
#include "xinterrupt_wrap.h"
#include "xipipsu.h"
#include "xparameters.h"
#include "xstatus.h"

int RpuIpiInitialize(XIpiPsu *InstancePtr, void *IntrHandler);
u32 RpuIpiGetInterruptStatus(XIpiPsu *InstancePtr);
void RpuIpiClearInterruptStatus(XIpiPsu *InstancePtr, u32 Mask);

#endif
