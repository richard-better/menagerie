#include <stdio.h>
int menagerie_admin_start(const char *prompt, char *const *arguments, FILE **channel, void **authorization);
void menagerie_admin_free(void *authorization);
