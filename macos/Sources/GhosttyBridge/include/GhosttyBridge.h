#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
typedef struct MGScreen MGScreen;
typedef struct {
  uint16_t x, y, width, flags;
  bool has_fg, has_bg;
  uint8_t fg[3], bg[3];
  char text[256];
} MGCell;
MGScreen *mg_screen_new(uint16_t cols, uint16_t rows);
void mg_screen_free(MGScreen *screen);
void mg_screen_write(MGScreen *screen, const uint8_t *bytes, size_t count);
bool mg_screen_resize(MGScreen *screen, uint16_t cols, uint16_t rows);
bool mg_screen_mode(MGScreen *screen, uint16_t mode);
bool mg_screen_snapshot(MGScreen *screen);
bool mg_screen_next(MGScreen *screen, MGCell *cell);
bool mg_screen_cursor(MGScreen *screen, uint16_t *x, uint16_t *y);
size_t mg_screen_replies(MGScreen *screen, uint8_t *buffer, size_t capacity);
int mg_pty_spawn(const char *program, char *const *argv, char *const *envp,
                 const char *directory, uint16_t cols, uint16_t rows, pid_t *pid, int *master);
bool mg_child_exited(pid_t pid);
