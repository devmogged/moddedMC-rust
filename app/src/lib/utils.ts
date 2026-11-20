import { type CxOptions, cx } from "class-variance-authority";
import { extendTailwindMerge } from "tailwind-merge";

// From T3 Code: register the extra text sizes so tailwind-merge doesn't read text-2xs as a color.
const twMerge = extendTailwindMerge({
  extend: { theme: { text: ["2xs", "3xs", "4xs", "5xs"] } },
});

export function cn(...inputs: CxOptions) {
  return twMerge(cx(inputs));
}
