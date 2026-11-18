/* WASI has no users: no home directory from the password database. */
#pragma once
#include <sys/types.h>
struct passwd { char *pw_name; char *pw_dir; };
static inline struct passwd *getpwuid(uid_t uid) { (void) uid; return 0; }
static inline uid_t getuid(void) { return 0; }
