/* Host functions the shim imports (Host.java); fabric-host/src/lib.rs defines them. */
#include <stdint.h>
int32_t mmc_never(void);
void mmc_register_command(void *name, int32_t length);
int32_t mmc_text(int32_t op, int32_t handle, void *buffer, int32_t capacity);
double mmc_player_num(int32_t player, int32_t field);
void mmc_send_message(int32_t player, void *utf8, int32_t length);
void mmc_teleport(int32_t player, void *dimension, int32_t length, double x, double y, double z, float yaw, float pitch);
int32_t mmc_tick_count(void);
int32_t mmc_world_count(void);
