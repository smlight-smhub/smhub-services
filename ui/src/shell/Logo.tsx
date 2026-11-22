/** The hub mark: a hexagon with a centre point, in the brand green. */
export function Logo({ small }: { small?: boolean }) {
  const size = small ? 28 : 36;
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" style={{ flex: 'none' }}>
      <rect width="32" height="32" rx="9" fill="var(--brand)" />
      <path d="M16 7l7.8 4.5v9L16 25l-7.8-4.5v-9z" fill="none" stroke="#fff" stroke-width="2.4" stroke-linejoin="round" />
      <circle cx="16" cy="16" r="2.6" fill="#fff" />
    </svg>
  );
}
