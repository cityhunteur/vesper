#include <sandbox.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <string.h>
int main(int argc, char **argv) {
  char *err = NULL;
  int pid = getpid();
  int rc = sandbox_init(argv[1], 0, &err);
  printf("sandbox_init rc=%d err=%s pid_before=%d pid_after=%d\n", rc, err ? err : "-", pid, getpid());
  int a = open(argv[2], O_RDONLY); printf("allowed open: %s\n", a >= 0 ? "ok" : strerror(errno));
  int d = open(argv[3], O_RDONLY); printf("denied open: %s\n", d >= 0 ? "OPENED" : strerror(errno));
  int w = open(argv[4], O_WRONLY|O_CREAT, 0600); printf("denied write: %s\n", w >= 0 ? "WROTE" : strerror(errno));
  int r2 = sandbox_init("(version 1)(allow default)", 0, &err); printf("second sandbox_init rc=%d (cannot widen)\n", r2);
  int d2 = open(argv[3], O_RDONLY); printf("denied open after second call: %s\n", d2 >= 0 ? "OPENED" : strerror(errno));
  return 0;
}
