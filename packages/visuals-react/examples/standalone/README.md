# Portable reference host

From the repository root:

```sh
node --preserve-symlinks node_modules/vite/bin/vite.js packages/visuals-react/examples/standalone --host 127.0.0.1 --port 5194
```

This example imports only the three core packages and React. Two independently
mounted clients share presentation controls, logical time, a static viewport,
and a 1,000-row analytical projection. Snapshots and recordings use an explicitly
in-memory demonstration adapter; reload resets it. Workshop supplies the durable
native adapter instead. This is a portability test, not native integration proof.
