/**
 * A small bar sparkline: one bar per value, scaled to the largest; an unknown value (null) is a hollow
 * stub, never a zero. Decorative; the number beside it is the reading.
 */
export function Spark({ values, label }: { values: (number | null)[]; label: string }) {
  const top = Math.max(1, ...values.map((v) => v ?? 0));
  return (
    <span className="ctl-spark" role="img" aria-label={label}>
      {values.map((v, i) => (v === null
        ? <i key={i} className="is-unknown" />
        : <i key={i} style={{ height: `${Math.max(v > 0 ? 8 : 2, (v / top) * 100)}%` }} />))}
    </span>
  );
}
