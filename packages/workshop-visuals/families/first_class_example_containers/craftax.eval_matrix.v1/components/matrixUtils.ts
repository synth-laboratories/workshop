import type { EvalMatrixPoint } from "../../../../runtime/types.ts";

export type MatrixSlice = {
  title?: string;
  achievements: string[];
  families?: string[];
  points: EvalMatrixPoint[];
};

export function chunkAchievements(ids: string[], cols = 6): string[][] {
  const rows: string[][] = [];
  for (let i = 0; i < ids.length; i += cols) {
    rows.push(ids.slice(i, i + cols));
  }
  return rows;
}

export function rateFor(
  point: EvalMatrixPoint,
  achievement: string
): number {
  return point.achievement_rates?.[achievement] ?? 0;
}
