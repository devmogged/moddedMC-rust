// T3 Code's components call gt-react for translations; this app is English only.
export function useGT() {
  return (text: string, _options?: Record<string, unknown>) => text;
}
