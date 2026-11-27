#include <errno.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <unistd.h>
extern char **environ;
int main(int argc, char **argv) {
  if (argc < 2) return 64;
  // posix_spawn creates the session and opens the slave, but Darwin requires
  // an explicit TIOCSCTTY before an interactive shell can use job control.
  if (ioctl(STDIN_FILENO, TIOCSCTTY, 0) != 0) { perror("TIOCSCTTY"); return 126; }
  execve(argv[1], argv + 1, environ);
  int error = errno;
  perror(argv[1]);
  return error == ENOENT ? 127 : 126;
}
