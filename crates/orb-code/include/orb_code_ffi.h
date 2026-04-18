#ifndef ORB_CODE_FFI_H
#define ORB_CODE_FFI_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct OrbCodeBuffer {
  uint8_t *data;
  size_t len;
} OrbCodeBuffer;

OrbCodeBuffer orb_code_generate_png_from_id(const char *orb_id, uint32_t image_size);
OrbCodeBuffer orb_code_generate_png_from_data(const char *input, uint32_t image_size);
char *orb_code_derive_id_from_data(const char *input);
char *orb_code_scan_png(const uint8_t *data, size_t len);
char *orb_code_scan_luma8(const uint8_t *data, size_t len, uint32_t width, uint32_t height);
bool orb_code_verify_png(const uint8_t *data, size_t len);
char *orb_code_last_error_message(void);
void orb_code_string_free(char *value);
void orb_code_buffer_free(OrbCodeBuffer buffer);

#endif
