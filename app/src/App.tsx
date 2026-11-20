import { useCallback, useState } from "react";
import { Launch, type LaunchResult } from "./Launch";
import { Shell } from "./Shell";

/** The launch screen first, then the app with what it found. */
export function App() {
  const [launch, setLaunch] = useState<LaunchResult | null>(null);
  const done = useCallback((result: LaunchResult) => setLaunch(result), []);
  return launch ? <Shell launch={launch} /> : <Launch onDone={done} />;
}
