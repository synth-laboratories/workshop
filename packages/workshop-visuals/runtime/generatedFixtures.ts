import {generateTrajectoryFixture} from "../src/swarm.ts";

/** Named, versioned fixtures only. These are never a missing-input fallback and
 * generate lazily, avoiding a multi-megabyte IPC binding or eager startup cost. */
export const generatedVisualFixtures:Readonly<Record<string,()=>unknown>>={
 "fixtures/trajectory-swarm-1000.v1":()=>generateTrajectoryFixture(1000,10),
};
