#include "AdminBridge.h"
#include <Security/Authorization.h>
#include <Security/AuthorizationTags.h>
#include <string.h>

int menagerie_admin_start(const char *prompt, char *const *arguments, FILE **channel, void **authorization) {
    AuthorizationRef auth = NULL;
    OSStatus status = AuthorizationCreate(NULL, NULL, 0, &auth);
    if (status) return status;
    AuthorizationItem item = { kAuthorizationRightExecute, 0, NULL, 0 };
    AuthorizationRights rights = { 1, &item };
    AuthorizationItem message = { kAuthorizationEnvironmentPrompt, strlen(prompt), (void *)prompt, 0 };
    AuthorizationEnvironment environment = { 1, &message };
    status = AuthorizationCopyRights(auth, &rights, &environment,
        kAuthorizationFlagInteractionAllowed | kAuthorizationFlagExtendRights | kAuthorizationFlagPreAuthorize, NULL);
    if (!status) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        status = AuthorizationExecuteWithPrivileges(auth, "/bin/sh", 0, arguments, channel);
#pragma clang diagnostic pop
    }
    if (status) AuthorizationFree(auth, kAuthorizationFlagDestroyRights);
    else *authorization = (void *)auth;
    return status;
}
void menagerie_admin_free(void *authorization) {
    AuthorizationFree(authorization, kAuthorizationFlagDestroyRights);
}
