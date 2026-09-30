/* SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Minimal libretro frontend used as a smoke test of a built core:
 *
 *   cc -O2 -o libretro_smoke scripts/libretro_smoke.c -ldl
 *   ./libretro_smoke path/to/openxeen_libretro.so [path/to/XEEN.CC]
 *
 * Loads the core with dlopen, runs the demo (no content) and, if given, the
 * content for 60 frames each, and checks that pictures and audio arrive.
 * Exit status 0 on success. Also usable under qemu-user for ARM builds.
 */
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct retro_game_info { const char *path; const void *data; size_t size; const char *meta; };
struct retro_system_info { const char *name, *version, *exts; bool need_fullpath, block_extract; };
struct retro_game_geometry { unsigned bw, bh, mw, mh; float aspect; };
struct retro_system_timing { double fps, sample_rate; };
struct retro_system_av_info { struct retro_game_geometry g; struct retro_system_timing t; };

static unsigned frames, colours_max;
static size_t audio_frames;
static uint32_t checksum;

static bool env(unsigned cmd, void *data) { (void)cmd; (void)data; return true; }
static void video(const void *data, unsigned w, unsigned h, size_t pitch) {
	const uint32_t *px = data;
	uint32_t seen[64]; unsigned n = 0;
	frames++;
	for (unsigned y = 0; y < h; y++)
		for (unsigned x = 0; x < w; x++) {
			uint32_t c = px[y * (pitch / 4) + x];
			checksum = checksum * 31 + c;
			unsigned i;
			for (i = 0; i < n && seen[i] != c; i++) {}
			if (i == n && n < 64) seen[n++] = c;
		}
	if (n > colours_max) colours_max = n;
}
static size_t audio_batch(const int16_t *d, size_t n) { (void)d; audio_frames += n; return n; }
static void audio_sample(int16_t l, int16_t r) { (void)l; (void)r; }
static void poll(void) {}
static int16_t state(unsigned p, unsigned d, unsigned i, unsigned id) { (void)p; (void)d; (void)i; (void)id; return 0; }

typedef bool (*env_fn)(unsigned, void *);
typedef void (*video_fn)(const void *, unsigned, unsigned, size_t);
typedef size_t (*batch_fn)(const int16_t *, size_t);
typedef void (*sample_fn)(int16_t, int16_t);
typedef void (*poll_fn)(void);
typedef int16_t (*state_fn)(unsigned, unsigned, unsigned, unsigned);

typedef unsigned (*api_version_t)(void);
typedef void (*get_info_t)(struct retro_system_info *);
typedef void (*get_av_t)(struct retro_system_av_info *);
typedef void (*set_env_t)(env_fn);
typedef void (*set_video_t)(video_fn);
typedef void (*set_batch_t)(batch_fn);
typedef void (*set_sample_t)(sample_fn);
typedef void (*set_poll_t)(poll_fn);
typedef void (*set_state_t)(state_fn);
typedef void (*void_t)(void);
typedef bool (*load_t)(const struct retro_game_info *);

#define SYM(T, name) T name = (T)dlsym(core, #name); \
	if (!name) { fprintf(stderr, "missing %s\n", #name); return 1; }

int main(int argc, char **argv) {
	if (argc < 2) { fprintf(stderr, "usage: %s core.so [content]\n", argv[0]); return 2; }
	void *core = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
	if (!core) { fprintf(stderr, "dlopen: %s\n", dlerror()); return 1; }
	SYM(api_version_t, retro_api_version);
	SYM(get_info_t, retro_get_system_info);
	SYM(get_av_t, retro_get_system_av_info);
	SYM(set_env_t, retro_set_environment);
	SYM(set_video_t, retro_set_video_refresh);
	SYM(set_batch_t, retro_set_audio_sample_batch);
	SYM(set_sample_t, retro_set_audio_sample);
	SYM(set_poll_t, retro_set_input_poll);
	SYM(set_state_t, retro_set_input_state);
	SYM(void_t, retro_init);
	SYM(void_t, retro_deinit);
	SYM(load_t, retro_load_game);
	SYM(void_t, retro_unload_game);
	SYM(void_t, retro_run);

	struct retro_system_info si; retro_get_system_info(&si);
	printf("core %s %s api %u exts %s\n", si.name, si.version, retro_api_version(), si.exts);
	retro_set_environment(env);
	retro_set_video_refresh(video);
	retro_set_audio_sample(audio_sample);
	retro_set_audio_sample_batch(audio_batch);
	retro_set_input_poll(poll);
	retro_set_input_state(state);
	retro_init();
	struct retro_system_av_info av; retro_get_system_av_info(&av);
	printf("av %ux%u aspect %.3f fps %.1f rate %.0f\n", av.g.bw, av.g.bh, av.g.aspect, av.t.fps, av.t.sample_rate);

	int rc = 0;
	for (int pass = 0; pass < (argc > 2 ? 2 : 1); pass++) {
		struct retro_game_info gi = { argc > 2 ? argv[2] : NULL, NULL, 0, NULL };
		frames = 0; colours_max = 0; audio_frames = 0; checksum = 0;
		if (!retro_load_game(pass ? &gi : NULL)) { fprintf(stderr, "load failed\n"); return 1; }
		for (int i = 0; i < 60; i++) retro_run();
		printf("%s: %u frames, up to %u colours, %zu audio frames, checksum %08x\n",
		       pass ? "content" : "demo", frames, colours_max, audio_frames, checksum);
		if (frames != 60 || colours_max < 4 || audio_frames != 60 * (size_t)(av.t.sample_rate / av.t.fps))
			rc = 1;
		retro_unload_game();
	}
	retro_deinit();
	dlclose(core);
	puts(rc ? "FAIL" : "PASS");
	return rc;
}
