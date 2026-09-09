// A real, deterministic PNG encoder for acceptance fixtures.
//
// Acceptance media has to be a genuine PNG — the native admission path checks
// the signature and Workshop's capture compares decoded pixels — but it must
// also be reproducible, so a rerun produces the same digests and a diff means a
// real change. Bytes vary per seed so a cut can be proved to carry its own
// image rather than a neighbour's.
import {deflateSync} from 'node:zlib';

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

export function deterministicPng(width, height, seed) {
  const chunk = (type, body) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(body.length);
    const typed = Buffer.concat([Buffer.from(type, 'ascii'), body]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(typed) >>> 0);
    return Buffer.concat([length, typed, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bit depth
  header[9] = 2; // truecolour
  const raw = Buffer.alloc((width * 3 + 1) * height);
  let offset = 0;
  for (let y = 0; y < height; y += 1) {
    raw[offset] = 0; // no filter: deterministic and trivially decodable
    offset += 1;
    for (let x = 0; x < width; x += 1) {
      raw[offset] = (x * 7 + seed * 31) & 0xff;
      raw[offset + 1] = (y * 5 + seed * 17) & 0xff;
      raw[offset + 2] = ((x ^ y) + seed * 3) & 0xff;
      offset += 3;
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw, {level: 9})),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}
