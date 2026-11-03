// Argon2id and repeated SHA-256 in plain JavaScript, without BigInt, the way a login page may do
// them, for comparing engines. Numbers of 64 bits are pairs of 32 bits, as JavaScript has nothing
// faster for them. Runs in browsers and in engines on their own. Only for measuring.
"use strict";

var Hash = (function () {
    var IV = [
        0xf3bcc908, 0x6a09e667, 0x84caa73b, 0xbb67ae85, 0xfe94f82b, 0x3c6ef372, 0x5f1d36f1, 0xa54ff53a,
        0xade682d1, 0x510e527f, 0x2b3e6c1f, 0x9b05688c, 0xfb41bd6b, 0x1f83d9ab, 0x137e2179, 0x5be0cd19,
    ];
    var SIGMA = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3,
        11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4,
        7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8,
        9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13,
        2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9,
        12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11,
        13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10,
        6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5,
        10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0,
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3,
    ];

    // A state of BLAKE2b: `h` holds eight numbers as low and high halves.
    function blakeStart(outLength) {
        var h = new Uint32Array(16);
        for (var i = 0; i < 16; i++) {
            h[i] = IV[i];
        }
        h[0] = (h[0] ^ 0x01010000 ^ outLength) >>> 0;
        return { h: h, buffer: new Uint8Array(128), filled: 0, count: 0, outLength: outLength };
    }

    var blakeV = new Uint32Array(32);
    var blakeM = new Uint32Array(32);

    function blakeG(a, b, c, d, xl, xh, yl, yh) {
        var v = blakeV;
        var l, h, tl, th;
        // a += b + x
        l = (v[a] >>> 0) + (v[b] >>> 0) + (xl >>> 0);
        h = v[a + 1] + v[b + 1] + xh + (l / 4294967296 | 0);
        v[a] = l;
        v[a + 1] = h;
        // d = (d ^ a) >>> 32
        tl = v[d] ^ v[a];
        th = v[d + 1] ^ v[a + 1];
        v[d] = th;
        v[d + 1] = tl;
        // c += d
        l = (v[c] >>> 0) + (v[d] >>> 0);
        v[c + 1] = v[c + 1] + v[d + 1] + (l / 4294967296 | 0);
        v[c] = l;
        // b = (b ^ c) >>> 24
        tl = v[b] ^ v[c];
        th = v[b + 1] ^ v[c + 1];
        v[b] = (tl >>> 24) | (th << 8);
        v[b + 1] = (th >>> 24) | (tl << 8);
        // a += b + y
        l = (v[a] >>> 0) + (v[b] >>> 0) + (yl >>> 0);
        h = v[a + 1] + v[b + 1] + yh + (l / 4294967296 | 0);
        v[a] = l;
        v[a + 1] = h;
        // d = (d ^ a) >>> 16
        tl = v[d] ^ v[a];
        th = v[d + 1] ^ v[a + 1];
        v[d] = (tl >>> 16) | (th << 16);
        v[d + 1] = (th >>> 16) | (tl << 16);
        // c += d
        l = (v[c] >>> 0) + (v[d] >>> 0);
        v[c + 1] = v[c + 1] + v[d + 1] + (l / 4294967296 | 0);
        v[c] = l;
        // b = (b ^ c) >>> 63
        tl = v[b] ^ v[c];
        th = v[b + 1] ^ v[c + 1];
        v[b] = (th >>> 31) | (tl << 1);
        v[b + 1] = (tl >>> 31) | (th << 1);
    }

    function blakeCompress(state, last) {
        var m = blakeM;
        var v = blakeV;
        var b = state.buffer;
        var h = state.h;
        var i;
        for (i = 0; i < 32; i++) {
            var at = i * 4;
            m[i] = b[at] | (b[at + 1] << 8) | (b[at + 2] << 16) | (b[at + 3] << 24);
        }
        for (i = 0; i < 16; i++) {
            v[i] = h[i];
            v[i + 16] = IV[i];
        }
        v[24] ^= state.count;
        if (last) {
            v[28] = ~v[28];
            v[29] = ~v[29];
        }
        for (var round = 0; round < 12; round++) {
            var s = round * 16;
            for (i = 0; i < 8; i++) {
                var x = SIGMA[s + 2 * i] * 2;
                var y = SIGMA[s + 2 * i + 1] * 2;
                if (i < 4) {
                    blakeG(i * 2, 8 + i * 2, 16 + i * 2, 24 + i * 2, m[x], m[x + 1], m[y], m[y + 1]);
                } else {
                    var j = i - 4;
                    blakeG(j * 2, 8 + ((j + 1) % 4) * 2, 16 + ((j + 2) % 4) * 2, 24 + ((j + 3) % 4) * 2,
                        m[x], m[x + 1], m[y], m[y + 1]);
                }
            }
        }
        for (i = 0; i < 16; i++) {
            h[i] = h[i] ^ v[i] ^ v[i + 16];
        }
    }

    function blakeAdd(state, input) {
        for (var i = 0; i < input.length; i++) {
            if (state.filled === 128) {
                state.count += 128;
                blakeCompress(state, false);
                state.filled = 0;
            }
            state.buffer[state.filled++] = input[i];
        }
    }

    function blakeEnd(state) {
        state.count += state.filled;
        state.buffer.fill(0, state.filled);
        blakeCompress(state, true);
        var out = new Uint8Array(state.outLength);
        for (var i = 0; i < state.outLength; i++) {
            out[i] = state.h[i >> 2] >>> (8 * (i & 3));
        }
        return out;
    }

    function le32(x) {
        return [x & 255, (x >>> 8) & 255, (x >>> 16) & 255, (x >>> 24) & 255];
    }

    function blakeLong(outLength, input) {
        var state = blakeStart(Math.min(outLength, 64));
        blakeAdd(state, le32(outLength));
        blakeAdd(state, input);
        if (outLength <= 64) {
            return blakeEnd(state);
        }
        var out = new Uint8Array(outLength);
        var v = blakeEnd(state);
        out.set(v.subarray(0, 32), 0);
        var written = 32;
        while (outLength - written > 64) {
            state = blakeStart(64);
            blakeAdd(state, v);
            v = blakeEnd(state);
            out.set(v.subarray(0, 32), written);
            written += 32;
        }
        state = blakeStart(outLength - written);
        blakeAdd(state, v);
        out.set(blakeEnd(state), written);
        return out;
    }

    // The high half of a product of two numbers of 32 bits.
    function highOfProduct(a, b) {
        var a0 = a & 0xffff;
        var a1 = a >>> 16;
        var b0 = b & 0xffff;
        var b1 = b >>> 16;
        var t = a0 * b0;
        var k = t >>> 16;
        t = a1 * b0 + k;
        var w1 = t & 0xffff;
        var w2 = t >>> 16;
        t = a0 * b1 + w1;
        return (a1 * b1 + w2 + (t >>> 16)) >>> 0;
    }

    // a = a + b + 2 * low(a) * low(b), in `r` at places a and b, as pairs of halves.
    function mix(r, a, b) {
        var al = r[a];
        var bl = r[b];
        var pl = Math.imul(al, bl);
        var ph = highOfProduct(al, bl);
        ph = (ph << 1) | (pl >>> 31);
        pl = pl << 1;
        var l = (al >>> 0) + (bl >>> 0);
        var carry = l / 4294967296 | 0;
        var low = (l >>> 0) + (pl >>> 0);
        carry += low / 4294967296 | 0;
        r[a + 1] = r[a + 1] + r[b + 1] + ph + carry;
        r[a] = low;
    }

    function argonG(r, a, b, c, d) {
        var tl, th;
        mix(r, a, b);
        tl = r[d] ^ r[a];
        th = r[d + 1] ^ r[a + 1];
        r[d] = th;
        r[d + 1] = tl;
        mix(r, c, d);
        tl = r[b] ^ r[c];
        th = r[b + 1] ^ r[c + 1];
        r[b] = (tl >>> 24) | (th << 8);
        r[b + 1] = (th >>> 24) | (tl << 8);
        mix(r, a, b);
        tl = r[d] ^ r[a];
        th = r[d + 1] ^ r[a + 1];
        r[d] = (tl >>> 16) | (th << 16);
        r[d + 1] = (th >>> 16) | (tl << 16);
        mix(r, c, d);
        tl = r[b] ^ r[c];
        th = r[b + 1] ^ r[c + 1];
        r[b] = (th >>> 31) | (tl << 1);
        r[b + 1] = (tl >>> 31) | (th << 1);
    }

    // The sixteen numbers at these places, given as places of pairs.
    function argonP(r, p) {
        argonG(r, p[0], p[4], p[8], p[12]);
        argonG(r, p[1], p[5], p[9], p[13]);
        argonG(r, p[2], p[6], p[10], p[14]);
        argonG(r, p[3], p[7], p[11], p[15]);
        argonG(r, p[0], p[5], p[10], p[15]);
        argonG(r, p[1], p[6], p[11], p[12]);
        argonG(r, p[2], p[7], p[8], p[13]);
        argonG(r, p[3], p[4], p[9], p[14]);
    }

    var COLUMNS = [];
    var ROWS = [];
    (function () {
        for (var i = 0; i < 8; i++) {
            var column = [];
            var row = [];
            for (var j = 0; j < 16; j++) {
                column.push((16 * i + j) * 2);
            }
            for (var k = 0; k < 8; k++) {
                row.push((i * 2 + k * 16) * 2);
                row.push((i * 2 + 1 + k * 16) * 2);
            }
            COLUMNS.push(column);
            ROWS.push(row);
        }
    })();

    var R = new Int32Array(256);
    var KEEP = new Int32Array(256);

    // Blocks are at places within `memory`, in numbers of 32 bits.
    function fillBlock(prevMem, prev, refMem, ref, nextMem, next, withXor) {
        var i;
        for (i = 0; i < 256; i++) {
            var x = prevMem[prev + i] ^ refMem[ref + i];
            R[i] = x;
            KEEP[i] = withXor ? x ^ nextMem[next + i] : x;
        }
        for (i = 0; i < 8; i++) {
            argonP(R, COLUMNS[i]);
        }
        for (i = 0; i < 8; i++) {
            argonP(R, ROWS[i]);
        }
        for (i = 0; i < 256; i++) {
            nextMem[next + i] = KEEP[i] ^ R[i];
        }
    }

    function argon2id(password, salt, secret, extra, passes, kib, lanes, tagLength) {
        var state = blakeStart(64);
        var head = [lanes, tagLength, kib, passes, 0x13, 2];
        var i;
        for (i = 0; i < head.length; i++) {
            blakeAdd(state, le32(head[i]));
        }
        var parts = [password, salt, secret, extra];
        for (i = 0; i < 4; i++) {
            blakeAdd(state, le32(parts[i].length));
            blakeAdd(state, parts[i]);
        }
        var h0 = blakeEnd(state);

        var segment = Math.floor(Math.max(kib, 8 * lanes) / (4 * lanes));
        var laneLength = segment * 4;
        var count = laneLength * lanes;
        var memory = new Int32Array(count * 256);
        var seed = new Uint8Array(72);
        seed.set(h0, 0);
        var lane, w;
        for (lane = 0; lane < lanes; lane++) {
            seed.set(le32(lane), 68);
            for (i = 0; i < 2; i++) {
                seed.set(le32(i), 64);
                var bytes = blakeLong(1024, seed);
                var place = (lane * laneLength + i) * 256;
                for (w = 0; w < 256; w++) {
                    var at = w * 4;
                    memory[place + w] = bytes[at] | (bytes[at + 1] << 8) | (bytes[at + 2] << 16) | (bytes[at + 3] << 24);
                }
            }
        }

        var zero = new Int32Array(256);
        var input = new Int32Array(256);
        var addresses = new Int32Array(256);
        function nextAddresses() {
            input[12]++;
            fillBlock(zero, 0, input, 0, addresses, 0, false);
            fillBlock(zero, 0, addresses, 0, addresses, 0, false);
        }

        for (var pass = 0; pass < passes; pass++) {
            for (var slice = 0; slice < 4; slice++) {
                for (lane = 0; lane < lanes; lane++) {
                    var independent = pass === 0 && slice < 2;
                    if (independent) {
                        input.fill(0);
                        input[0] = pass;
                        input[2] = lane;
                        input[4] = slice;
                        input[6] = count;
                        input[8] = passes;
                        input[10] = 2;
                    }
                    var start = pass === 0 && slice === 0 ? 2 : 0;
                    if (independent && start === 2) {
                        nextAddresses();
                    }
                    var current = lane * laneLength + slice * segment + start;
                    var previous = current % laneLength === 0 ? current + laneLength - 1 : current - 1;
                    for (i = start; i < segment; i++, current++, previous++) {
                        if (current % laneLength === 1) {
                            previous = current - 1;
                        }
                        var low, high;
                        if (independent) {
                            if (i % 128 === 0) {
                                nextAddresses();
                            }
                            low = addresses[(i % 128) * 2] >>> 0;
                            high = addresses[(i % 128) * 2 + 1] >>> 0;
                        } else {
                            low = memory[previous * 256] >>> 0;
                            high = memory[previous * 256 + 1] >>> 0;
                        }
                        var refLane = pass === 0 && slice === 0 ? lane : high % lanes;
                        var same = refLane === lane;
                        var area;
                        if (pass === 0) {
                            if (slice === 0) {
                                area = i - 1;
                            } else if (same) {
                                area = slice * segment + i - 1;
                            } else {
                                area = slice * segment - (i === 0 ? 1 : 0);
                            }
                        } else if (same) {
                            area = laneLength - segment + i - 1;
                        } else {
                            area = laneLength - segment - (i === 0 ? 1 : 0);
                        }
                        var position = area - 1 - highOfProduct(area, highOfProduct(low, low));
                        var from = pass !== 0 && slice !== 3 ? (slice + 1) * segment : 0;
                        var refIndex = (from + position) % laneLength;
                        fillBlock(memory, previous * 256, memory, (refLane * laneLength + refIndex) * 256,
                            memory, current * 256, pass !== 0);
                    }
                }
            }
        }

        var last = new Int32Array(256);
        for (lane = 0; lane < lanes; lane++) {
            var end = (lane * laneLength + laneLength - 1) * 256;
            for (w = 0; w < 256; w++) {
                last[w] ^= memory[end + w];
            }
        }
        var lastBytes = new Uint8Array(1024);
        for (w = 0; w < 1024; w++) {
            lastBytes[w] = last[w >> 2] >>> (8 * (w & 3));
        }
        return blakeLong(tagLength, lastBytes);
    }

    var SHA_K = new Int32Array([
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ]);
    var SHA_W = new Int32Array(64);

    // One message of at most 55 bytes, which is all that is needed here.
    function sha256Into(out, input, length) {
        var w = SHA_W;
        var i;
        for (i = 0; i < 16; i++) {
            w[i] = 0;
        }
        for (i = 0; i < length; i++) {
            w[i >> 2] |= input[i] << (24 - 8 * (i & 3));
        }
        w[length >> 2] |= 0x80 << (24 - 8 * (length & 3));
        w[15] = length * 8;
        for (i = 16; i < 64; i++) {
            var x = w[i - 15];
            var y = w[i - 2];
            var s0 = ((x >>> 7) | (x << 25)) ^ ((x >>> 18) | (x << 14)) ^ (x >>> 3);
            var s1 = ((y >>> 17) | (y << 15)) ^ ((y >>> 19) | (y << 13)) ^ (y >>> 10);
            w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0;
        }
        var a = 0x6a09e667, b = 0xbb67ae85 | 0, c = 0x3c6ef372, d = 0xa54ff53a | 0;
        var e = 0x510e527f, f = 0x9b05688c | 0, g = 0x1f83d9ab, h = 0x5be0cd19;
        for (i = 0; i < 64; i++) {
            var t1 = (h + (((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7)))
                + ((e & f) ^ (~e & g)) + SHA_K[i] + w[i]) | 0;
            var t2 = ((((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10)))
                + ((a & b) ^ (a & c) ^ (b & c))) | 0;
            h = g;
            g = f;
            f = e;
            e = (d + t1) | 0;
            d = c;
            c = b;
            b = a;
            a = (t1 + t2) | 0;
        }
        var sum = [(a + 0x6a09e667) | 0, (b + 0xbb67ae85) | 0, (c + 0x3c6ef372) | 0, (d + 0xa54ff53a) | 0,
            (e + 0x510e527f) | 0, (f + 0x9b05688c) | 0, (g + 0x1f83d9ab) | 0, (h + 0x5be0cd19) | 0];
        for (i = 0; i < 8; i++) {
            out[i * 4] = sum[i] >>> 24;
            out[i * 4 + 1] = sum[i] >>> 16;
            out[i * 4 + 2] = sum[i] >>> 8;
            out[i * 4 + 3] = sum[i];
        }
    }

    function filled(length, value) {
        var out = new Uint8Array(length);
        out.fill(value);
        return out;
    }

    function hex(bytes, count) {
        var out = "";
        for (var i = 0; i < count; i++) {
            out += (bytes[i] < 16 ? "0" : "") + bytes[i].toString(16);
        }
        return out;
    }

    // RFC 9106, 5.3, and the usual one for SHA-256.
    function selfTest() {
        var tag = argon2id(filled(32, 1), filled(16, 2), filled(8, 3), filled(12, 4), 3, 32, 4, 32);
        var digest = new Uint8Array(32);
        sha256Into(digest, [97, 98, 99], 3);
        return hex(tag, 32) === "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659"
            && hex(digest, 32) === "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    }

    function now() {
        return typeof performance !== "undefined" ? performance.now() : Date.now();
    }

    function chars(text) {
        var out = [];
        for (var i = 0; i < text.length; i++) {
            out.push(text.charCodeAt(i));
        }
        return out;
    }

    function runArgon2(kib, passes) {
        var nothing = new Uint8Array(0);
        return argon2id(chars("password"), chars("somesaltsomesalt"), nothing, nothing, passes, kib, 1, 32);
    }

    function runSha256(times) {
        var digest = filled(32, 7);
        for (var i = 0; i < times; i++) {
            sha256Into(digest, digest, 32);
        }
        return digest;
    }

    // Measures and tells each result by `report(name, milliseconds, firstBytesOfResult)`. What would
    // take too long in a slow engine is worked out from the smallest, which it scales with, and
    // the report says so.
    function runAll(report, sizes) {
        report("self test " + (selfTest() ? "right" : "WRONG"), 0, "");
        var start = now();
        var digest = runSha256(10000);
        var shaMs = now() - start;
        report("sha256 x 10000", shaMs, hex(digest, 8));
        var perBlock = 0;
        for (var i = 0; i < sizes.length; i++) {
            var kib = sizes[i][0];
            var passes = sizes[i][1];
            var name = "argon2id " + kib + " KiB, " + passes + " passes";
            if (perBlock * kib * passes > 20000) {
                report(name + " (estimated)", perBlock * kib * passes, "");
                continue;
            }
            start = now();
            var tag = runArgon2(kib, passes);
            var ms = now() - start;
            perBlock = ms / (kib * passes);
            report(name, ms, hex(tag, 8));
        }
    }

    return { runAll: runAll, SIZES: [[1024, 1], [19456, 2], [65536, 3]] };
})();
