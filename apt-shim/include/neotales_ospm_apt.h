#ifndef NEOTALES_OSPM_APT_H
#define NEOTALES_OSPM_APT_H

#include <stdint.h>

/* ABI version is incremented for incompatible changes. */
uint32_t neotales_ospm_apt_abi_version(void);

/* Return 1 if installed, 0 if not installed or unknown, -1 on error.
 * name must point to a NUL-terminated UTF-8 string for the duration of the call.
 * Callers must serialize all libapt-pkg access in the process.
 */
int32_t neotales_ospm_apt_is_installed(const char *name);

#endif
