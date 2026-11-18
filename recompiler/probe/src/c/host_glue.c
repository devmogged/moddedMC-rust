// Host side of the probe: a real recompiled mod gets WIT glue here instead.
#include <stdint.h>
#include <stdio.h>

void mmc_log(int32_t value) { printf("mmc_log %d\n", value); }
void mmc_print(void *utf8, int32_t length) { printf("mmc_print %.*s\n", (int) length, (const char *) utf8); }
int32_t mmc_never(void) { return 0; }

int teavm_main(int argc, char **argv);
int32_t probe_add(int32_t a, int32_t b);

// The component's lifecycle: bring the Java runtime up once, then call exports later.
int main(void) {
    teavm_main(0, 0);
    int32_t first = probe_add(2, 3);
    int32_t second = probe_add(40, 2);
    printf("results %d %d\n", first, second);
    return first == 5 && second == 42 ? 0 : 1;
}
