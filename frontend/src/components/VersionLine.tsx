import { useDesktop } from '../lib/bridge';

export function VersionLine({ className = '' }: { className?: string }): React.ReactElement | null {
  const { info } = useDesktop();
  if (!info) return null;

  return (
    <p
      className={`font-mono text-[11px] leading-relaxed text-slate-400 ${className}`}
      title={`AutoDOP ${info.version} — build ${info.build}`}
    >
      v{info.version} · {info.build}
    </p>
  );
}

export default VersionLine;
