#include "GhosttyBridge.h"
#include <ghostty/vt.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <spawn.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <unistd.h>
#include <util.h>
struct MGScreen {
  GhosttyTerminal terminal;
  GhosttyRenderState render;
  GhosttyRenderStateRowIterator rows;
  GhosttyRenderStateRowCells cells;
  uint16_t x, y;
  bool in_row;
  uint8_t replies[65536];
  size_t reply_count;
};
static void reply(GhosttyTerminal terminal, void *context, const uint8_t *data, size_t len) {
  (void)terminal;
  MGScreen *s = context;
  size_t available = sizeof(s->replies) - s->reply_count;
  if (len <= available) { memcpy(s->replies + s->reply_count, data, len); s->reply_count += len; }
}
MGScreen *mg_screen_new(uint16_t cols, uint16_t rows) {
  if (!cols || !rows || cols > 512 || rows > 256) return NULL;
  MGScreen *s = calloc(1, sizeof(*s));
  if (!s) return NULL;
  size_t zero = 0, max_bytes = 4 * 1024 * 1024;
  size_t max_lines = 2000;
  if (ghostty_terminal_new(NULL, &s->terminal, cols, rows) != GHOSTTY_SUCCESS ||
      ghostty_render_state_new(NULL, &s->render) != GHOSTTY_SUCCESS ||
      ghostty_render_state_row_iterator_new(NULL, &s->rows) != GHOSTTY_SUCCESS ||
      ghostty_render_state_row_cells_new(NULL, &s->cells) != GHOSTTY_SUCCESS ||
      ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_APC_MAX_BYTES, &zero) != GHOSTTY_SUCCESS ||
      ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_UNKNOWN_MAX_BYTES, &zero) != GHOSTTY_SUCCESS ||
      ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, &max_bytes) != GHOSTTY_SUCCESS ||
      ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES, &max_lines) != GHOSTTY_SUCCESS) {
    mg_screen_free(s); return NULL;
  }
  ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_USERDATA, s);
  ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_WRITE_PTY, (const void *)reply);
  return s;
}
void mg_screen_free(MGScreen *s) {
  if (!s) return;
  ghostty_render_state_row_cells_free(s->cells);
  ghostty_render_state_row_iterator_free(s->rows);
  ghostty_render_state_free(s->render);
  ghostty_terminal_free(s->terminal);
  free(s);
}
void mg_screen_write(MGScreen *s, const uint8_t *bytes, size_t count) { ghostty_terminal_vt_write(s->terminal, bytes, count); }
bool mg_screen_resize(MGScreen *s, uint16_t cols, uint16_t rows) {
  return cols && rows && cols <= 512 && rows <= 256 && ghostty_terminal_resize(s->terminal, cols, rows, 8, 16) == GHOSTTY_SUCCESS;
}
bool mg_screen_mode(MGScreen *s, uint16_t mode) {
  GhosttyTerminalModeConfig c = {.mode = ghostty_mode_new(mode, false)};
  return ghostty_terminal_get(s->terminal, GHOSTTY_TERMINAL_DATA_MODE, &c) == GHOSTTY_SUCCESS && c.value;
}
bool mg_screen_snapshot(MGScreen *s) {
  s->x = 0; s->y = 0; s->in_row = false;
  return ghostty_render_state_update(s->render, s->terminal) == GHOSTTY_SUCCESS &&
    ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &s->rows) == GHOSTTY_SUCCESS;
}
bool mg_screen_next(MGScreen *s, MGCell *out) {
  while (true) {
    if (!s->in_row) {
      if (!ghostty_render_state_row_iterator_next(s->rows)) return false;
      if (ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &s->cells) != GHOSTTY_SUCCESS) return false;
      s->x = 0; s->in_row = true;
    }
    if (!ghostty_render_state_row_cells_next(s->cells)) { s->in_row = false; s->y++; continue; }
    uint16_t x = s->x++;
    GhosttyCell raw = 0;
    GhosttyCellWide wide = GHOSTTY_CELL_WIDE_NARROW;
    ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &raw);
    ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &wide);
    if (wide == GHOSTTY_CELL_WIDE_SPACER_TAIL) continue;
    memset(out, 0, sizeof(*out)); out->x = x; out->y = s->y; out->width = wide == GHOSTTY_CELL_WIDE_WIDE ? 2 : 1;
    GhosttyBuffer text = {.ptr = (uint8_t *)out->text, .cap = sizeof(out->text) - 1};
    if (ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &text) != GHOSTTY_SUCCESS) strcpy(out->text, "\xef\xbf\xbd");
    else if (!text.len) strcpy(out->text, " ");
    GhosttyColorRgb fg, bg;
    out->has_fg = ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, &fg) == GHOSTTY_SUCCESS;
    out->has_bg = ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, &bg) == GHOSTTY_SUCCESS;
    if (out->has_fg) { out->fg[0] = fg.r; out->fg[1] = fg.g; out->fg[2] = fg.b; }
    if (out->has_bg) { out->bg[0] = bg.r; out->bg[1] = bg.g; out->bg[2] = bg.b; }
    GhosttyStyle style;
    ghostty_style_default(&style);
    ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &style);
    out->flags = style.flags;
    return true;
  }
}
bool mg_screen_cursor(MGScreen *s, uint16_t *x, uint16_t *y) {
  GhosttyRenderStateCursor c = {.size = sizeof(c)};
  if (ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_CURSOR, &c) != GHOSTTY_SUCCESS || !c.visible || !c.viewport_has_value) return false;
  *x = c.viewport_x; *y = c.viewport_y; return true;
}
size_t mg_screen_replies(MGScreen *s, uint8_t *buffer, size_t capacity) {
  size_t count = s->reply_count < capacity ? s->reply_count : capacity;
  memcpy(buffer, s->replies, count); memmove(s->replies, s->replies + count, s->reply_count - count); s->reply_count -= count; return count;
}
int mg_pty_spawn(const char *program, char *const *argv, char *const *envp, const char *directory,
                 uint16_t cols, uint16_t rows, pid_t *pid, int *master) {
  int slave = -1;
  char name[PATH_MAX];
  struct winsize size = {.ws_col = cols, .ws_row = rows};
  if (openpty(master, &slave, name, NULL, &size) != 0) return errno;
  fcntl(*master, F_SETFD, FD_CLOEXEC); fcntl(slave, F_SETFD, FD_CLOEXEC);
  fcntl(*master, F_SETFL, O_NONBLOCK); fcntl(*master, F_SETNOSIGPIPE, 1);
  posix_spawnattr_t attr; posix_spawn_file_actions_t actions;
  posix_spawnattr_init(&attr); posix_spawn_file_actions_init(&actions);
  posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSID | POSIX_SPAWN_CLOEXEC_DEFAULT);
  posix_spawn_file_actions_addopen(&actions, 0, name, O_RDWR, 0);
  posix_spawn_file_actions_adddup2(&actions, 0, 1); posix_spawn_file_actions_adddup2(&actions, 0, 2);
  if (directory) posix_spawn_file_actions_addchdir(&actions, directory);
  int result = posix_spawn(pid, program, &actions, &attr, argv, envp);
  posix_spawn_file_actions_destroy(&actions); posix_spawnattr_destroy(&attr); close(slave);
  if (result) { close(*master); *master = -1; }
  return result;
}
bool mg_child_exited(pid_t pid) {
  siginfo_t info = {0};
  return waitid(P_PID, (id_t)pid, &info, WEXITED | WNOHANG | WNOWAIT) == 0 && info.si_pid == pid;
}
