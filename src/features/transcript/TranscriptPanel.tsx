import { useEffect, useRef, useState } from "react";
import { locate } from "@/features/preview/sync";
import { usePreviewStore } from "@/features/preview/store";
import { useTimelineStore } from "@/features/timeline/store";
import type { AssetSummary, TranscriptReport } from "@/ipc";
import { cn } from "@/shared/lib/utils";
import { messageOf, notify } from "@/shared/notify";
import { btn, btnPrimary, input } from "@/shared/ui/styles";
import { activePhraseIndex, formatStamp, timelinePositionOf } from "./match";
import { LANGUAGES, MODELS, useTranscriptStore } from "./store";

interface Props {
  asset: AssetSummary | null;
}

/** Transcript of the selected clip: read it, jump to a phrase, follow it while playing. */
export function TranscriptPanel({ asset }: Props) {
  const report = useTranscriptStore((s) => (asset ? s.byAsset[asset.id] : undefined));
  const running = useTranscriptStore((s) => s.running);
  const language = useTranscriptStore((s) => s.language);
  const model = useTranscriptStore((s) => s.model);
  const setLanguage = useTranscriptStore((s) => s.setLanguage);
  const setModel = useTranscriptStore((s) => s.setModel);
  const run = useTranscriptStore((s) => s.run);

  if (!asset) return <Notice>Select a clip in the Project panel first.</Notice>;
  if (!asset.has_audio) {
    return <Notice>This clip has no audio track, so there is nothing to transcribe.</Notice>;
  }

  const busy = running !== null;
  const thisOneRuns = running?.assetId === asset.id;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="space-y-2 border-b border-black/40 p-3">
        <div className="flex gap-2">
          <select
            aria-label="Language"
            value={language}
            onChange={(e) => setLanguage(e.target.value)}
            disabled={busy}
            className={cn(input, "min-w-0 flex-1")}
          >
            {LANGUAGES.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
          <select
            aria-label="Speech model"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            disabled={busy}
            className={cn(input, "min-w-0 flex-1")}
          >
            {MODELS.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <button
          className={cn(btnPrimary, "w-full")}
          disabled={busy}
          onClick={() => void run(asset)}
        >
          {thisOneRuns ? <Working since={running.since} /> : report ? "Transcribe again" : "Transcribe"}
        </button>
        {thisOneRuns && (
          <p className="text-[11px] leading-relaxed text-neutral-500">
            The first time a model is used it is downloaded, which can take a few minutes.
          </p>
        )}
      </div>
      {report ? <Phrases asset={asset} report={report} /> : <Empty busy={thisOneRuns} />}
    </div>
  );
}

function Notice({ children }: { children: string }) {
  return <p className="p-3 text-[11px] leading-relaxed text-neutral-500">{children}</p>;
}

function Empty({ busy }: { busy: boolean }) {
  if (busy) return null;
  return (
    <p className="p-3 text-[11px] leading-relaxed text-neutral-500">
      No transcript yet. Transcribing runs on this computer; nothing is uploaded.
    </p>
  );
}

function Working({ since }: { since: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return <>Transcribing… {Math.max(0, Math.round((now - since) / 1000))} s</>;
}

interface PhrasesProps {
  asset: AssetSummary;
  report: TranscriptReport;
}

function Phrases({ asset, report }: PhrasesProps) {
  const cuts = useTimelineStore((s) => s.cuts);
  const seek = useTimelineStore((s) => s.seek);
  const playing = usePreviewStore((s) => s.playing);
  // Only the playhead's clip can say which phrase is being spoken, and only if it is this asset.
  const active = useTimelineStore((s) => {
    const here = locate(s.cuts, s.playhead);
    if (!here || here.cut.asset !== asset.id) return null;
    return activePhraseIndex(report.segments, here.sourceTime);
  });

  const activeRef = useRef<HTMLLIElement>(null);
  useEffect(() => {
    if (playing) activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active, playing]);

  const jump = (start: number) => {
    const position = timelinePositionOf(cuts, asset.id, start);
    if (position === null) notify.info("That moment is not on the timeline");
    else seek(position);
  };

  const copy = () => {
    navigator.clipboard.writeText(report.full_text).then(
      () => notify.info("Transcript copied"),
      (error: unknown) => notify.error(messageOf(error)),
    );
  };

  return (
    <>
      <div className="flex items-center justify-between px-3 py-2 text-[11px] text-neutral-500">
        <span>
          {report.segments.length} phrases · {report.language} · {report.model}
        </span>
        <button className={btn} onClick={copy} title="Copy the whole text">
          Copy
        </button>
      </div>
      <ul className="min-h-0 flex-1 select-text overflow-y-auto px-1 pb-2">
        {report.segments.map((phrase, index) => (
          <li key={`${phrase.start}-${index}`} ref={index === active ? activeRef : undefined}>
            <button
              onClick={() => jump(phrase.start)}
              title="Go to this phrase on the timeline"
              className={cn(
                "flex w-full gap-2 rounded px-2 py-1 text-left text-xs leading-snug",
                index === active ? "bg-sky-700/40 text-neutral-50" : "text-neutral-300 hover:bg-neutral-700/40",
              )}
            >
              <span className="w-10 shrink-0 font-mono text-[11px] text-sky-400">
                {formatStamp(phrase.start)}
              </span>
              <span>{phrase.text.trim()}</span>
            </button>
          </li>
        ))}
      </ul>
    </>
  );
}
