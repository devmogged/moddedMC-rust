// Host side of the probe's @Import: a real recompiled mod gets WIT glue here instead.
#include <stdint.h>
#include <stdio.h>

void mmc_log(int32_t value) { printf("mmc_log %d\n", value); }
