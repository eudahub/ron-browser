/* Argon2id and repeated SHA-256, for comparing how fast JavaScript engines and WebAssembly get
 * through what a login page may ask of them. Runs one lane after the other, on one thread, as
 * JavaScript does. Built for a program with BENCH_NATIVE, and otherwise as WebAssembly with no C
 * library at all. Only for measuring: use a real library for real passwords. */

#include <stddef.h>
#include <stdint.h>

#ifdef BENCH_NATIVE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#define EXPORT
#else
#define EXPORT __attribute__((visibility("default")))
extern unsigned char __heap_base[];

void *memcpy(void *to, const void *from, size_t count)
{
    unsigned char *out = to;
    const unsigned char *in = from;
    while (count--) {
        *out++ = *in++;
    }
    return to;
}

void *memset(void *to, int value, size_t count)
{
    unsigned char *out = to;
    while (count--) {
        *out++ = (unsigned char)value;
    }
    return to;
}
#endif

typedef struct {
    uint64_t v[128];
} Block;

/* BLAKE2b */

static const uint64_t blake_iv[8] = {
    0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL, 0x3c6ef372fe94f82bULL, 0xa54ff53a5f1d36f1ULL,
    0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL, 0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL,
};

static const uint8_t sigma[12][16] = {
    {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15},
    {14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3},
    {11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4},
    {7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8},
    {9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13},
    {2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9},
    {12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11},
    {13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10},
    {6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5},
    {10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0},
    {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15},
    {14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3},
};

typedef struct {
    uint64_t h[8];
    uint64_t count;
    uint8_t buffer[128];
    size_t filled;
    size_t out_length;
} Blake;

static uint64_t rotr64(uint64_t x, int n)
{
    return (x >> n) | (x << (64 - n));
}

static uint64_t load64(const uint8_t *p)
{
    uint64_t x = 0;
    for (int i = 7; i >= 0; --i) {
        x = (x << 8) | p[i];
    }
    return x;
}

static void store64(uint8_t *p, uint64_t x)
{
    for (int i = 0; i < 8; ++i) {
        p[i] = (uint8_t)(x >> (8 * i));
    }
}

static void store32(uint8_t *p, uint32_t x)
{
    for (int i = 0; i < 4; ++i) {
        p[i] = (uint8_t)(x >> (8 * i));
    }
}

static void blake_compress(Blake *state, const uint8_t *block, int last)
{
    uint64_t m[16];
    uint64_t v[16];
    for (int i = 0; i < 16; ++i) {
        m[i] = load64(block + 8 * i);
    }
    for (int i = 0; i < 8; ++i) {
        v[i] = state->h[i];
        v[i + 8] = blake_iv[i];
    }
    v[12] ^= state->count;
    if (last) {
        v[14] = ~v[14];
    }
#define BLAKE_G(a, b, c, d, x, y)                                                                                      \
    v[a] += v[b] + (x);                                                                                                \
    v[d] = rotr64(v[d] ^ v[a], 32);                                                                                    \
    v[c] += v[d];                                                                                                      \
    v[b] = rotr64(v[b] ^ v[c], 24);                                                                                    \
    v[a] += v[b] + (y);                                                                                                \
    v[d] = rotr64(v[d] ^ v[a], 16);                                                                                    \
    v[c] += v[d];                                                                                                      \
    v[b] = rotr64(v[b] ^ v[c], 63);
    for (int r = 0; r < 12; ++r) {
        const uint8_t *s = sigma[r];
        BLAKE_G(0, 4, 8, 12, m[s[0]], m[s[1]])
        BLAKE_G(1, 5, 9, 13, m[s[2]], m[s[3]])
        BLAKE_G(2, 6, 10, 14, m[s[4]], m[s[5]])
        BLAKE_G(3, 7, 11, 15, m[s[6]], m[s[7]])
        BLAKE_G(0, 5, 10, 15, m[s[8]], m[s[9]])
        BLAKE_G(1, 6, 11, 12, m[s[10]], m[s[11]])
        BLAKE_G(2, 7, 8, 13, m[s[12]], m[s[13]])
        BLAKE_G(3, 4, 9, 14, m[s[14]], m[s[15]])
    }
    for (int i = 0; i < 8; ++i) {
        state->h[i] ^= v[i] ^ v[i + 8];
    }
}

static void blake_start(Blake *state, size_t out_length)
{
    for (int i = 0; i < 8; ++i) {
        state->h[i] = blake_iv[i];
    }
    state->h[0] ^= 0x01010000ULL ^ out_length;
    state->count = 0;
    state->filled = 0;
    state->out_length = out_length;
}

static void blake_add(Blake *state, const uint8_t *in, size_t length)
{
    while (length > 0) {
        if (state->filled == 128) {
            state->count += 128;
            blake_compress(state, state->buffer, 0);
            state->filled = 0;
        }
        size_t take = 128 - state->filled;
        if (take > length) {
            take = length;
        }
        memcpy(state->buffer + state->filled, in, take);
        state->filled += take;
        in += take;
        length -= take;
    }
}

static void blake_end(Blake *state, uint8_t *out)
{
    state->count += state->filled;
    memset(state->buffer + state->filled, 0, 128 - state->filled);
    blake_compress(state, state->buffer, 1);
    uint8_t whole[64];
    for (int i = 0; i < 8; ++i) {
        store64(whole + 8 * i, state->h[i]);
    }
    memcpy(out, whole, state->out_length);
}

static void blake_long(uint8_t *out, uint32_t out_length, const uint8_t *in, size_t in_length)
{
    uint8_t length_bytes[4];
    store32(length_bytes, out_length);
    Blake state;
    blake_start(&state, out_length < 64 ? out_length : 64);
    blake_add(&state, length_bytes, 4);
    blake_add(&state, in, in_length);
    if (out_length <= 64) {
        blake_end(&state, out);
        return;
    }
    uint8_t v[64];
    blake_end(&state, v);
    memcpy(out, v, 32);
    uint32_t written = 32;
    while (out_length - written > 64) {
        blake_start(&state, 64);
        blake_add(&state, v, 64);
        blake_end(&state, v);
        memcpy(out + written, v, 32);
        written += 32;
    }
    blake_start(&state, out_length - written);
    blake_add(&state, v, 64);
    blake_end(&state, out + written);
}

/* Argon2id */

static uint64_t mix(uint64_t x, uint64_t y)
{
    return x + y + 2 * (x & 0xFFFFFFFFULL) * (y & 0xFFFFFFFFULL);
}

#define ARGON_G(a, b, c, d)                                                                                            \
    a = mix(a, b);                                                                                                     \
    d = rotr64(d ^ a, 32);                                                                                             \
    c = mix(c, d);                                                                                                     \
    b = rotr64(b ^ c, 24);                                                                                             \
    a = mix(a, b);                                                                                                     \
    d = rotr64(d ^ a, 16);                                                                                             \
    c = mix(c, d);                                                                                                     \
    b = rotr64(b ^ c, 63);

#define ARGON_P(v0, v1, v2, v3, v4, v5, v6, v7, v8, v9, v10, v11, v12, v13, v14, v15)                                  \
    ARGON_G(v0, v4, v8, v12)                                                                                           \
    ARGON_G(v1, v5, v9, v13)                                                                                           \
    ARGON_G(v2, v6, v10, v14)                                                                                          \
    ARGON_G(v3, v7, v11, v15)                                                                                          \
    ARGON_G(v0, v5, v10, v15)                                                                                          \
    ARGON_G(v1, v6, v11, v12)                                                                                          \
    ARGON_G(v2, v7, v8, v13)                                                                                           \
    ARGON_G(v3, v4, v9, v14)

static void fill_block(const Block *previous, const Block *reference, Block *next, int with_xor)
{
    Block r;
    Block keep;
    for (int i = 0; i < 128; ++i) {
        r.v[i] = previous->v[i] ^ reference->v[i];
        keep.v[i] = r.v[i];
    }
    if (with_xor) {
        for (int i = 0; i < 128; ++i) {
            keep.v[i] ^= next->v[i];
        }
    }
    for (int i = 0; i < 8; ++i) {
        uint64_t *v = r.v + 16 * i;
        ARGON_P(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11], v[12], v[13], v[14], v[15])
    }
    for (int i = 0; i < 8; ++i) {
        uint64_t *v = r.v;
        ARGON_P(v[i * 2], v[i * 2 + 1], v[i * 2 + 16], v[i * 2 + 17], v[i * 2 + 32], v[i * 2 + 33], v[i * 2 + 48],
                v[i * 2 + 49], v[i * 2 + 64], v[i * 2 + 65], v[i * 2 + 80], v[i * 2 + 81], v[i * 2 + 96], v[i * 2 + 97],
                v[i * 2 + 112], v[i * 2 + 113])
    }
    for (int i = 0; i < 128; ++i) {
        next->v[i] = keep.v[i] ^ r.v[i];
    }
}

static void next_addresses(Block *input, Block *addresses, const Block *zero)
{
    input->v[6]++;
    fill_block(zero, input, addresses, 0);
    fill_block(zero, addresses, addresses, 0);
}

static Block *memory_blocks;
static uint32_t memory_capacity;

static Block *blocks_for(uint32_t count)
{
    if (count > memory_capacity) {
#ifdef BENCH_NATIVE
        free(memory_blocks);
        memory_blocks = malloc(count * sizeof(Block));
#else
        uintptr_t start = (uintptr_t)__heap_base;
        uintptr_t end = start + (uintptr_t)count * sizeof(Block);
        uintptr_t have = __builtin_wasm_memory_size(0) * 65536u;
        if (end > have) {
            __builtin_wasm_memory_grow(0, (end - have + 65535u) / 65536u);
        }
        memory_blocks = (Block *)start;
#endif
        memory_capacity = count;
    }
    return memory_blocks;
}

static void argon2id(uint8_t *tag, uint32_t tag_length, const uint8_t *password, uint32_t password_length,
                     const uint8_t *salt, uint32_t salt_length, const uint8_t *secret, uint32_t secret_length,
                     const uint8_t *extra, uint32_t extra_length, uint32_t passes, uint32_t kib, uint32_t lanes)
{
    uint8_t seed[72];
    uint8_t number[4];
    Blake state;
    blake_start(&state, 64);
    const uint32_t head[6] = {lanes, tag_length, kib, passes, 0x13, 2};
    for (int i = 0; i < 6; ++i) {
        store32(number, head[i]);
        blake_add(&state, number, 4);
    }
    const uint8_t *parts[4] = {password, salt, secret, extra};
    const uint32_t lengths[4] = {password_length, salt_length, secret_length, extra_length};
    for (int i = 0; i < 4; ++i) {
        store32(number, lengths[i]);
        blake_add(&state, number, 4);
        blake_add(&state, parts[i], lengths[i]);
    }
    blake_end(&state, seed);

    uint32_t least = 8 * lanes;
    uint32_t segment = (kib < least ? least : kib) / (4 * lanes);
    uint32_t lane_length = segment * 4;
    uint32_t count = lane_length * lanes;
    Block *memory = blocks_for(count);
    uint8_t bytes[1024];
    for (uint32_t lane = 0; lane < lanes; ++lane) {
        store32(seed + 68, lane);
        for (uint32_t i = 0; i < 2; ++i) {
            store32(seed + 64, i);
            blake_long(bytes, 1024, seed, 72);
            for (int w = 0; w < 128; ++w) {
                memory[lane * lane_length + i].v[w] = load64(bytes + 8 * w);
            }
        }
    }

    Block zero;
    memset(&zero, 0, sizeof zero);
    for (uint32_t pass = 0; pass < passes; ++pass) {
        for (uint32_t slice = 0; slice < 4; ++slice) {
            for (uint32_t lane = 0; lane < lanes; ++lane) {
                const int independent = pass == 0 && slice < 2;
                Block input;
                Block addresses;
                if (independent) {
                    memset(&input, 0, sizeof input);
                    input.v[0] = pass;
                    input.v[1] = lane;
                    input.v[2] = slice;
                    input.v[3] = count;
                    input.v[4] = passes;
                    input.v[5] = 2;
                }
                uint32_t start = pass == 0 && slice == 0 ? 2 : 0;
                if (independent && start == 2) {
                    next_addresses(&input, &addresses, &zero);
                }
                uint32_t current = lane * lane_length + slice * segment + start;
                uint32_t previous = current % lane_length == 0 ? current + lane_length - 1 : current - 1;
                for (uint32_t i = start; i < segment; ++i, ++current, ++previous) {
                    if (current % lane_length == 1) {
                        previous = current - 1;
                    }
                    uint64_t pseudo;
                    if (independent) {
                        if (i % 128 == 0) {
                            next_addresses(&input, &addresses, &zero);
                        }
                        pseudo = addresses.v[i % 128];
                    } else {
                        pseudo = memory[previous].v[0];
                    }
                    uint32_t ref_lane = (uint32_t)((pseudo >> 32) % lanes);
                    if (pass == 0 && slice == 0) {
                        ref_lane = lane;
                    }
                    const int same = ref_lane == lane;
                    uint32_t area;
                    if (pass == 0) {
                        if (slice == 0) {
                            area = i - 1;
                        } else if (same) {
                            area = slice * segment + i - 1;
                        } else {
                            area = slice * segment - (i == 0 ? 1 : 0);
                        }
                    } else if (same) {
                        area = lane_length - segment + i - 1;
                    } else {
                        area = lane_length - segment - (i == 0 ? 1 : 0);
                    }
                    uint64_t low = pseudo & 0xFFFFFFFFULL;
                    low = (low * low) >> 32;
                    uint32_t position = area - 1 - (uint32_t)(((uint64_t)area * low) >> 32);
                    uint32_t from = pass != 0 && slice != 3 ? (slice + 1) * segment : 0;
                    uint32_t ref_index = (from + position) % lane_length;
                    fill_block(&memory[previous], &memory[ref_lane * lane_length + ref_index], &memory[current], pass != 0);
                }
            }
        }
    }

    Block last = memory[lane_length - 1];
    for (uint32_t lane = 1; lane < lanes; ++lane) {
        for (int w = 0; w < 128; ++w) {
            last.v[w] ^= memory[lane * lane_length + lane_length - 1].v[w];
        }
    }
    for (int w = 0; w < 128; ++w) {
        store64(bytes + 8 * w, last.v[w]);
    }
    blake_long(tag, tag_length, bytes, 1024);
}

/* SHA-256 */

static const uint32_t sha_k[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
};

static uint32_t rotr32(uint32_t x, int n)
{
    return (x >> n) | (x << (32 - n));
}

/* One message of at most 55 bytes, which is all that is needed here. */
static void sha256(uint8_t *out, const uint8_t *in, uint32_t length)
{
    uint32_t h[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
    uint8_t block[64];
    memset(block, 0, 64);
    memcpy(block, in, length);
    block[length] = 0x80;
    block[62] = (uint8_t)((length * 8) >> 8);
    block[63] = (uint8_t)(length * 8);
    uint32_t w[64];
    for (int i = 0; i < 16; ++i) {
        w[i] = (uint32_t)block[i * 4] << 24 | (uint32_t)block[i * 4 + 1] << 16 | (uint32_t)block[i * 4 + 2] << 8 |
               block[i * 4 + 3];
    }
    for (int i = 16; i < 64; ++i) {
        uint32_t s0 = rotr32(w[i - 15], 7) ^ rotr32(w[i - 15], 18) ^ (w[i - 15] >> 3);
        uint32_t s1 = rotr32(w[i - 2], 17) ^ rotr32(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], k = h[7];
    for (int i = 0; i < 64; ++i) {
        uint32_t t1 = k + (rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25)) + ((e & f) ^ (~e & g)) + sha_k[i] + w[i];
        uint32_t t2 = (rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22)) + ((a & b) ^ (a & c) ^ (b & c));
        k = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }
    const uint32_t add[8] = {a, b, c, d, e, f, g, k};
    for (int i = 0; i < 8; ++i) {
        uint32_t x = h[i] + add[i];
        out[i * 4] = (uint8_t)(x >> 24);
        out[i * 4 + 1] = (uint8_t)(x >> 16);
        out[i * 4 + 2] = (uint8_t)(x >> 8);
        out[i * 4 + 3] = (uint8_t)x;
    }
}

/* What the hosts call */

static uint8_t result[32];

EXPORT uint32_t result_byte(uint32_t index)
{
    return result[index];
}

/* RFC 9106, 5.3. Returns 1 when all is right. */
EXPORT int self_test(void)
{
    static const uint8_t wanted[32] = {0x0d, 0x64, 0x0d, 0xf5, 0x8d, 0x78, 0x76, 0x6c, 0x08, 0xc0, 0x37,
                                       0xa3, 0x4a, 0x8b, 0x53, 0xc9, 0xd0, 0x1e, 0xf0, 0x45, 0x2d, 0x75,
                                       0xb6, 0x5e, 0xb5, 0x25, 0x20, 0xe9, 0x6b, 0x01, 0xe6, 0x59};
    uint8_t password[32], salt[16], secret[8], extra[12];
    memset(password, 1, 32);
    memset(salt, 2, 16);
    memset(secret, 3, 8);
    memset(extra, 4, 12);
    argon2id(result, 32, password, 32, salt, 16, secret, 8, extra, 12, 3, 32, 4);
    for (int i = 0; i < 32; ++i) {
        if (result[i] != wanted[i]) {
            return 0;
        }
    }
    static const uint8_t abc_wanted[4] = {0xba, 0x78, 0x16, 0xbf};
    sha256(result, (const uint8_t *)"abc", 3);
    for (int i = 0; i < 4; ++i) {
        if (result[i] != abc_wanted[i]) {
            return 0;
        }
    }
    return 1;
}

EXPORT void run_argon2(uint32_t kib, uint32_t passes)
{
    static const uint8_t password[8] = {'p', 'a', 's', 's', 'w', 'o', 'r', 'd'};
    static const uint8_t salt[16] = {'s', 'o', 'm', 'e', 's', 'a', 'l', 't', 's', 'o', 'm', 'e', 's', 'a', 'l', 't'};
    argon2id(result, 32, password, 8, salt, 16, salt, 0, salt, 0, passes, kib, 1);
}

EXPORT void run_sha256(uint32_t times)
{
    uint8_t digest[32];
    memset(digest, 7, 32);
    for (uint32_t i = 0; i < times; ++i) {
        sha256(digest, digest, 32);
    }
    memcpy(result, digest, 32);
}

#ifdef BENCH_NATIVE
static double now_ms(void)
{
    struct timespec time;
    clock_gettime(CLOCK_MONOTONIC, &time);
    return (double)time.tv_sec * 1000.0 + (double)time.tv_nsec / 1e6;
}

static void show_result(void)
{
    for (int i = 0; i < 8; ++i) {
        printf("%02x", result[i]);
    }
}

int main(void)
{
    printf("self test: %s\n", self_test() ? "right" : "WRONG");
    const uint32_t sizes[][2] = {{1024, 1}, {19456, 2}, {65536, 3}};
    for (int i = 0; i < 3; ++i) {
        double start = now_ms();
        run_argon2(sizes[i][0], sizes[i][1]);
        printf("argon2id %u KiB, %u passes: %.1f ms ", sizes[i][0], sizes[i][1], now_ms() - start);
        show_result();
        printf("\n");
    }
    double start = now_ms();
    run_sha256(1000000);
    printf("sha256 x 1000000: %.1f ms ", now_ms() - start);
    show_result();
    printf("\n");
    return 0;
}
#endif
