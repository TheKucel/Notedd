#ifndef NOTEDD_CORE_H
#define NOTEDD_CORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// The input buffers are borrowed for the call. Callback buffers are valid only until the callback returns.
typedef struct {
    const uint8_t *data;
    size_t length;
} NoteddSlice;

typedef void (*NoteddTextCallback)(void *context, NoteddSlice text);
typedef void (*NoteddNoteCallback)(void *context, NoteddSlice text, size_t color);

enum {
    NOTEDD_OK = 0,
    NOTEDD_INVALID = 1,
    NOTEDD_NOT_FOUND = 2,
    NOTEDD_PERMISSION = 3,
    NOTEDD_CONFLICT = 4,
    NOTEDD_IO = 5
};

bool notedd_decode_note(NoteddSlice raw, void *context, NoteddNoteCallback callback);
void notedd_note_title(NoteddSlice text, void *context, NoteddTextCallback callback);
// has_original distinguishes new notes from existing empty files. The callback runs only after a write.
int32_t notedd_save_note(const char *path, NoteddSlice original, bool has_original, NoteddSlice text, size_t color,
                         void *context, NoteddTextCallback callback);
int32_t notedd_write_new(const char *path, NoteddSlice data);
void notedd_read_state(const char *folder, void *context, NoteddTextCallback callback);
void notedd_write_state(const char *folder, NoteddSlice id);

enum {
    NOTEDD_PLAIN, NOTEDD_HEADING, NOTEDD_QUOTE, NOTEDD_FENCE, NOTEDD_CODE_BLOCK, NOTEDD_IMAGE
};

enum {
    NOTEDD_NO_LIST, NOTEDD_BULLET, NOTEDD_NUMBERED, NOTEDD_TASK
};

enum {
    NOTEDD_MARK, NOTEDD_BOLD, NOTEDD_ITALIC, NOTEDD_STRIKE, NOTEDD_HIGHLIGHT, NOTEDD_INLINE_CODE, NOTEDD_LINK, NOTEDD_URL
};

typedef struct {
    size_t start;
    size_t end;
    uint8_t style;
    bool hidden;
} NoteddSpan;

typedef struct {
    size_t start;
    size_t end;
    NoteddSlice target;
} NoteddLink;

typedef struct {
    uint8_t block;
    uint8_t heading_level;
    bool opening_fence;
    size_t block_marker_length;
    NoteddSlice image_path;
    uint8_t list;
    size_t list_start;
    size_t list_end;
    bool checked;
    size_t task_offset;
} NoteddLine;

typedef void (*NoteddLineCallback)(void *context, NoteddLine line,
                                   const NoteddSpan *spans, size_t span_count,
                                   const NoteddLink *links, size_t link_count);

// One callback per line, including empty lines. All offsets within the line are UTF-16 indices.
void notedd_markdown(NoteddSlice text, void *context, NoteddLineCallback callback);

typedef void (*NoteddEditCallback)(void *context, size_t start, size_t end, NoteddSlice marker, bool wrap);
void notedd_format_edit(NoteddSlice line, size_t start, size_t end, uint8_t style, bool simple, void *context, NoteddEditCallback callback);

typedef void (*NoteddReplacementCallback)(void *context, size_t start, size_t end, NoteddSlice before, NoteddSlice after);
void notedd_simple_replacement(NoteddSlice text, size_t start, size_t end, bool erase,
                               void *context, NoteddReplacementCallback callback);

#endif
