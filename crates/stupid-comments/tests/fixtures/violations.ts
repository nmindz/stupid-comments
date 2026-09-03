export function calc(a: number, b: number): number {
  // Adds a and b
  const sum = a + b;
  // This function implements the accumulation strategy described in PRD-4471.
  // The reason we do it this way is that the original approach used a reducer
  // which allocated an intermediate array on every call, and profiling showed
  // that this dominated the hot path during batch import, so we replaced it
  // with a direct addition, which avoids the allocation entirely and keeps
  // the function monomorphic for the JIT.
  return sum;
}
