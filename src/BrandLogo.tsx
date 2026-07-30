import { useEffect, useState } from "react";
import logoLight from "./assets/logo-light.svg";
import logoDark from "./assets/logo-dark.svg";

function useSystemDark(): boolean {
  const [dark, setDark] = useState(
    () =>
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches,
  );

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener("change", onChange);
    setDark(mq.matches);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  return dark;
}

/** App mark that follows the OS light/dark appearance. */
export default function BrandLogo({
  className = "",
  alt = "transcribe",
}: {
  className?: string;
  alt?: string;
}) {
  const dark = useSystemDark();
  return (
    <img
      className={className}
      src={dark ? logoDark : logoLight}
      alt={alt}
      draggable={false}
    />
  );
}

export { useSystemDark };
