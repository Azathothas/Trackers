/* libc-surface.c - the libc calls a static pacman actually needs.
 *
 * pacman calls getpwnam() in four places to resolve DownloadUser
 * (lib/libalpm/{dload.c,handle.c,sandbox.c,util.c}) and resolves mirror
 * hostnames through libcurl, which reaches getaddrinfo(). Both are the
 * classic "static glibc needs NSS shared objects at runtime" calls, so a
 * toolchain that cannot link them statically cannot build pacman-static.
 *
 * Printed output is the evidence; the exit code is the assertion.
 *   0  every probed call linked and returned a defined answer
 *   1  linked, but a call misbehaved at runtime
 */
#include <stdio.h>
#include <string.h>
#include <pwd.h>
#include <grp.h>
#include <netdb.h>
#include <unistd.h>
#include <sys/utsname.h>

int main(void) {
    struct utsname u;
    int rc = 0;

    if (uname(&u) == 0)
        printf("uname.machine=%s\n", u.machine);
    else
        { printf("uname=FAILED\n"); rc = 1; }

    /* root is present in every /etc/passwd; a NULL here means the lookup
       path is broken, not that the user is absent. */
    struct passwd *pw = getpwnam("root");
    printf("getpwnam(root)=%s uid=%ld\n",
           pw ? pw->pw_name : "NULL", pw ? (long)pw->pw_uid : -1L);
    if (!pw) rc = 1;

    struct group *gr = getgrnam("root");
    printf("getgrnam(root)=%s\n", gr ? gr->gr_name : "NULL");

    /* getaddrinfo on a numeric literal exercises the resolver's link path
       without needing a network or a working /etc/resolv.conf. */
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC;
    hints.ai_flags  = AI_NUMERICHOST;
    int gai = getaddrinfo("127.0.0.1", NULL, &hints, &res);
    printf("getaddrinfo(127.0.0.1)=%s\n", gai == 0 ? "ok" : gai_strerror(gai));
    if (gai != 0) rc = 1;
    if (res) freeaddrinfo(res);

    printf("verdict=%s\n", rc == 0 ? "ALL_CALLS_RESOLVED" : "DEGRADED");
    return rc;
}
