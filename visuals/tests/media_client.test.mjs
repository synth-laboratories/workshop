import assert from "node:assert/strict";
import test from "node:test";

import {
  MEDIA_CACHE_LIMIT,
  MEDIA_PRELOAD_AHEAD,
  MEDIA_PRELOAD_BEHIND,
  NO_MEDIA,
  createMediaClient,
  isCasDigest,
  mediaRefFrom
} from "../runtime/mediaClient.ts";

// Hex with letters in it: a digest built only from decimal digits would be
// unchanged by `toUpperCase`, which would make the case assertion below vacuous.
const digest = (seed) => `ab${String(seed).padStart(62, "c")}`;

function recordingTransport(options = {}) {
  const asked = [];
  const gates = new Map();
  const transport = async (casDigest) => {
    asked.push(casDigest);
    if (options.gate) {
      await new Promise((resolve) => gates.set(casDigest, resolve));
    }
    if (options.failOn?.includes(casDigest)) {
      throw new Error(`refused ${casDigest}`);
    }
    return {
      casDigest,
      mediaType: "image/png",
      byteSize: 1024,
      width: 768,
      height: 768,
      dataUrl: `data:image/png;base64,${casDigest.slice(0, 8)}`
    };
  };
  return { transport, asked, release: (id) => gates.get(id)?.() };
}

test("a digest is 64 hex characters, and a producer's 16-character label is not one", () => {
  assert.equal(isCasDigest(digest(1)), true);
  // The digest observed on real frame events. It names the frame; it does not
  // address it, and asking the host for it must never look like a valid request.
  assert.equal(isCasDigest("4e27ac3b1f0a9d55"), false);
  assert.equal(isCasDigest(digest(1).toUpperCase()), false);
  assert.equal(isCasDigest(undefined), false);
});

test("a frame payload yields a reference, and a media block without one yields null", () => {
  const ref = mediaRefFrom({
    step: 7,
    format: "png",
    media: {
      casDigest: digest(7),
      mediaType: "image/png",
      width: 768,
      height: 768,
      producerDigest: "4e27ac3b1f0a9d55"
    }
  });
  assert.equal(ref.casDigest, digest(7));
  assert.equal(ref.width, 768);
  // Provenance travels; it is never mistaken for the address.
  assert.equal(ref.producerDigest, "4e27ac3b1f0a9d55");
  assert.notEqual(ref.producerDigest, ref.casDigest);

  // A refused frame keeps its event and offers no reference to load.
  assert.equal(mediaRefFrom({ step: 3, mediaError: { reason: "refused" } }), null);
  assert.equal(mediaRefFrom({ media: { producerDigest: "4e27ac3b1f0a9d55" } }), null);
  assert.equal(mediaRefFrom(null), null);
});

test("a client with no transport says so instead of hanging", async () => {
  assert.equal(NO_MEDIA.peek(digest(1)), undefined);
  await assert.rejects(() => NO_MEDIA.load(digest(1)), /no media transport/);
  assert.equal(await NO_MEDIA.warm([digest(1)], 0), undefined);
});

test("a digest names bytes, so it is fetched once and then cached", async () => {
  const { transport, asked } = recordingTransport();
  const client = createMediaClient(transport);
  const first = await client.load(digest(1));
  const second = await client.load(digest(1));
  assert.equal(first.dataUrl, second.dataUrl);
  assert.deepEqual(asked, [digest(1)], "an immutable object was fetched twice");
  assert.equal(client.peek(digest(1)).byteSize, 1024);
});

test("concurrent requests for one digest share a single fetch", async () => {
  const { transport, asked, release } = recordingTransport({ gate: true });
  const client = createMediaClient(transport);
  const pending = [client.load(digest(2)), client.load(digest(2)), client.load(digest(2))];
  await new Promise((resolve) => setImmediate(resolve));
  release(digest(2));
  const loaded = await Promise.all(pending);
  assert.equal(asked.length, 1, "a scrub past one frame issued three requests for it");
  assert.equal(new Set(loaded.map((item) => item.dataUrl)).size, 1);
});

test("warming loads the selection plus a bounded window, never the timeline", async () => {
  const { transport, asked } = recordingTransport();
  const client = createMediaClient(transport);
  const timeline = Array.from({ length: 200 }, (_, index) => digest(index));
  const selected = await client.warm(timeline, 100);
  assert.equal(selected.casDigest, digest(100));
  // The selection resolves first: a scrubber must not wait on its lookahead.
  assert.equal(asked[0], digest(100));
  await new Promise((resolve) => setImmediate(resolve));
  const window = MEDIA_PRELOAD_AHEAD + MEDIA_PRELOAD_BEHIND + 1;
  assert.equal(asked.length, window, `warmed ${asked.length} of 200 frames`);
  assert.ok(asked.includes(digest(100 - MEDIA_PRELOAD_BEHIND)));
  assert.ok(asked.includes(digest(100 + MEDIA_PRELOAD_AHEAD)));
  assert.ok(!asked.includes(digest(0)), "warming reached the whole timeline");
});

test("the decoded cache is bounded so a long scrub cannot grow without limit", async () => {
  const { transport } = recordingTransport();
  const client = createMediaClient(transport);
  for (let index = 0; index < MEDIA_CACHE_LIMIT + 10; index += 1) {
    await client.load(digest(index));
  }
  assert.equal(client.peek(digest(MEDIA_CACHE_LIMIT + 9)).byteSize, 1024, "the newest was evicted");
  assert.equal(client.peek(digest(0)), undefined, "the oldest was never evicted");
});

test("a refused frame is recorded by name rather than left as a blank tile", async () => {
  const { transport } = recordingTransport({ failOn: [digest(5)] });
  const client = createMediaClient(transport);
  await assert.rejects(() => client.load(digest(5)), /refused/);
  assert.match(client.failures().get(digest(5)), /refused/);
  // A failure in the preload window never breaks the selection.
  const selected = await client.warm([digest(4), digest(5), digest(6)], 0);
  assert.equal(selected.casDigest, digest(4));
});

test("a warmed selection that itself fails resolves to undefined, not a rejection", async () => {
  const { transport } = recordingTransport({ failOn: [digest(9)] });
  const client = createMediaClient(transport);
  assert.equal(await client.warm([digest(9)], 0), undefined);
  assert.match(client.failures().get(digest(9)), /refused/);
});
