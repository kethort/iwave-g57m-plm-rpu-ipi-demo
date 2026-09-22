#include "xplm_hello_spam_module.h"

#include "xplmi_debug.h"
#include "xplmi_task.h"
#include "xstatus.h"

/*
 * Limit the output so this test does not permanently monopolize PLM.
 * Increase this value after confirming the module works.
 */
#define XPLM_HELLO_SPAM_COUNT	(1000U)

static XPlmi_TaskNode *HelloSpamTask;
static u32 HelloSpamCounter;

static int XPlm_HelloSpamTaskHandler(void *Arg)
{
	(void)Arg;

	XPlmi_Printf(
		DEBUG_PRINT_ALWAYS,
		"PLM USER MODULE: HELLO WORLD %lu\r\n",
		(unsigned long)HelloSpamCounter
	);

	HelloSpamCounter++;

	if ((HelloSpamCounter < XPLM_HELLO_SPAM_COUNT) &&
	    (HelloSpamTask != NULL)) {
		XPlmi_TaskTriggerNow(HelloSpamTask);
	}

	return XST_SUCCESS;
}

int XPlm_HelloSpamModuleInit(void)
{
	HelloSpamCounter = 0U;

	XPlmi_Printf(
		DEBUG_PRINT_ALWAYS,
		"PLM HELLO-SPAM MODULE INITIALIZING\r\n"
	);

	HelloSpamTask = XPlmi_TaskCreate(
		XPLM_TASK_PRIORITY_1,
		XPlm_HelloSpamTaskHandler,
		NULL
	);

	if (HelloSpamTask == NULL) {
		XPlmi_Printf(
			DEBUG_PRINT_ALWAYS,
			"PLM HELLO-SPAM TASK CREATE FAILED\r\n"
		);

		return XST_FAILURE;
	}

	XPlmi_TaskTriggerNow(HelloSpamTask);

	XPlmi_Printf(
		DEBUG_PRINT_ALWAYS,
		"PLM HELLO-SPAM MODULE REGISTERED\r\n"
	);

	return XST_SUCCESS;
}