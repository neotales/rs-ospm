#ifndef NEOTALES_OSPM_PACMAN_H
#define NEOTALES_OSPM_PACMAN_H
#include <stdint.h>

uint32_t neotales_ospm_pacman_abi_version(void);
/* Callback string pointers are borrowed for the duration of the call.
 * Return 0 to continue or nonzero to stop. The library path must be trusted.
 * Return 0 on success or -1 on failure. */
int32_t neotales_ospm_pacman_list(
    const char *library, const char *root, const char *dbpath,
    int32_t (*callback)(void *context, const char *name, const char *version),
    void *context);
#endif
