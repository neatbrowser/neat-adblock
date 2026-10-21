/* neat_adblock.h - C ABI around adblock-rust for NEAT Browser.
 *
 * Ownership:
 *   - NeatEngine* from neat_engine_create  -> neat_engine_free
 *   - char* returned by this library       -> neat_string_free
 *   - strings inside NeatCheckResult       -> neat_check_result_free
 * Input strings are UTF-8, NUL terminated, only borrowed during the call.
 * Every function is safe to call concurrently on the same engine.
 */
#ifndef NEAT_ADBLOCK_H
#define NEAT_ADBLOCK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NEAT_ABI_VERSION 1u

#define NEAT_OK            0
#define NEAT_ERR_ARGS     (-1) /* null pointer, invalid UTF-8 or invalid JSON */
#define NEAT_ERR_URL      (-2) /* a URL could not be parsed */
#define NEAT_ERR_INTERNAL (-3) /* internal error (a panic was caught) */

#define NEAT_ACTION_ALLOW    0
#define NEAT_ACTION_BLOCK    1
#define NEAT_ACTION_REDIRECT 2

typedef struct NeatEngine NeatEngine;

typedef struct NeatCheckResult {
    int32_t action;        /* NEAT_ACTION_* */
    char*   redirect;      /* REDIRECT: replacement resource (data: URL) to serve instead */
    char*   rewritten_url; /* ALLOW only: non-NULL when the URL must be rewritten ($removeparam) */
} NeatCheckResult;

uint32_t neat_abi_version(void);

/* Builds the engines from a directory holding a neat-filters release (manifest.json + the files
 * it lists). Verify the files' hashes before calling. Returns NULL on failure and, when
 * error_out is not NULL, an error message (release with neat_string_free). Slow: call it off the
 * UI thread. */
NeatEngine* neat_engine_create(const char* dir, char** error_out);
void        neat_engine_free(NeatEngine* engine);
void        neat_string_free(char* s);

/* request_type: document, subdocument, stylesheet, image, media, font, script,
 * xmlhttprequest, ping, websocket, object, other. method may be NULL.
 * `result` is always initialised; release it with neat_check_result_free. */
int32_t neat_check_request(const NeatEngine* engine, const char* url, const char* source_url,
                           const char* request_type, const char* method, NeatCheckResult* result);
void    neat_check_result_free(NeatCheckResult* result);

/* For NewWindowRequested: 1 = block the new window, 0 = allow, negative = error. */
int32_t neat_check_popup(const NeatEngine* engine, const char* target_url, const char* opener_url);

/* Extra CSP directives for a document/subdocument response. 1 = csp_out set, 0 = none. */
int32_t neat_get_csp(const NeatEngine* engine, const char* url, const char* source_url,
                     const char* request_type, char** csp_out);

/* JSON: {"hide_selectors":[],"procedural_actions":[],"exceptions":[],
 *        "injected_script":"","generichide":false} */
int32_t neat_cosmetic_resources(const NeatEngine* engine, const char* url, char** json_out);

/* Inputs: JSON arrays of strings. Output: JSON array of CSS selectors. */
int32_t neat_hidden_class_id_selectors(const NeatEngine* engine, const char* classes_json,
                                       const char* ids_json, const char* exceptions_json,
                                       char** json_out);

#ifdef __cplusplus
}
#endif
#endif /* NEAT_ADBLOCK_H */
