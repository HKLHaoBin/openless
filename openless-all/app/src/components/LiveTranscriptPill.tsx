import { animate, motion, useMotionValue, useReducedMotion, useTransform } from 'framer-motion';
import { useEffect, useMemo, useState } from 'react';
import './CapsuleStyles.css';
import {
  INSERT_TEXT_MOTION,
  clampInsertContentWidth,
  diffInsertUnits,
  firstBornIndex,
  planCapsuleInsertMotion,
  planCharDelays,
  prefixWidths,
  type InsertUnit,
} from '../lib/insertTextAnimation';
import { Icon } from './Icon';

export type LiveTranscriptTone = 'frost' | 'dark';

export interface LiveTranscriptPillProps {
  text: string;
  tone?: LiveTranscriptTone;
  stageWidth: number;
  maxWidth: number;
  minWidth?: number;
  height?: number;
  onCancel?: () => void;
  onConfirm?: () => void;
  cancelEnabled?: boolean;
  confirmEnabled?: boolean;
  cancelLabel?: string;
  confirmLabel?: string;
  controlSize?: number;
}

const FONT_SIZE = 16;
const FONT_WEIGHT = 500;
const DEFAULT_PILL_HEIGHT = 40;

let measureCanvas: HTMLCanvasElement | null = null;

function getSansFont(): string {
  if (typeof window === 'undefined') return 'system-ui, sans-serif';
  const token = getComputedStyle(document.documentElement)
    .getPropertyValue('--ol-font-sans')
    .trim();
  return token || 'system-ui, sans-serif';
}

function measureGlyphWidths(glyphs: string[]): number[] {
  if (glyphs.length === 0) return [];
  if (typeof document === 'undefined') {
    return glyphs.map((glyph) => Math.max(FONT_SIZE * 0.92, glyph.length * FONT_SIZE));
  }
  if (!measureCanvas) measureCanvas = document.createElement('canvas');
  const ctx = measureCanvas.getContext('2d');
  if (!ctx) return glyphs.map(() => FONT_SIZE);
  ctx.font = `${FONT_WEIGHT} ${FONT_SIZE}px ${getSansFont()}`;
  return glyphs.map((glyph) => {
    const raw = ctx.measureText(glyph === ' ' ? '\u00a0' : glyph).width;
    return Math.max(1, raw);
  });
}

export function LiveTranscriptPill({
  text,
  tone = 'frost',
  stageWidth,
  maxWidth,
  minWidth = INSERT_TEXT_MOTION.minWidth,
  height = DEFAULT_PILL_HEIGHT,
  onCancel,
  onConfirm,
  cancelEnabled = true,
  confirmEnabled = true,
  cancelLabel,
  confirmLabel,
  controlSize = 28,
}: LiveTranscriptPillProps) {
  const reduceMotion = useReducedMotion();
  const hasControls = Boolean(onCancel || onConfirm);
  const padX = INSERT_TEXT_MOTION.padX + (hasControls ? controlSize + 6 : 0);
  const pillMinWidth = hasControls ? Math.max(minWidth, controlSize * 2 + 72) : minWidth;
  const [units, setUnits] = useState<InsertUnit[]>(() => diffInsertUnits([], text));
  const [seenText, setSeenText] = useState(text);
  if (text !== seenText) {
    setSeenText(text);
    setUnits((prev) => diffInsertUnits(prev, text));
  }

  const widths = useMemo(() => measureGlyphWidths(units.map((unit) => unit.text)), [units]);
  const contentWidth = useMemo(
    () =>
      clampInsertContentWidth(
        widths.reduce((sum, width) => sum + width, 0),
        maxWidth,
        padX,
      ),
    [widths, maxWidth, padX],
  );
  const originIndex = firstBornIndex(units);
  const starts = useMemo(() => prefixWidths(widths), [widths]);
  const originX = (starts[originIndex] ?? 0) + (widths[originIndex] ?? 0) / 2;
  const delays = useMemo(() => planCharDelays(widths, originIndex), [widths, originIndex]);
  const plan = useMemo(
    () =>
      planCapsuleInsertMotion({
        stageWidth,
        contentWidth,
        originX,
        minWidth: pillMinWidth,
        maxWidth,
        padX,
      }),
    [stageWidth, contentWidth, originX, pillMinWidth, maxWidth, padX],
  );

  const widthMv = useMotionValue(pillMinWidth);
  const rightEdgeMv = useMotionValue(stageWidth / 2 + pillMinWidth / 2);
  const leftMv = useTransform(
    [rightEdgeMv, widthMv],
    ([right, width]) => Number(right) - Number(width),
  );
  const trackRightMv = useTransform(rightEdgeMv, (right) => stageWidth - Number(right) + padX);

  useEffect(() => {
    if (reduceMotion) {
      widthMv.set(plan.width);
      rightEdgeMv.set(plan.rightEdge);
      return undefined;
    }
    const widthAnim = animate(widthMv, plan.width, {
      type: 'spring',
      ...INSERT_TEXT_MOTION.widthSpring,
      delay: plan.widthDelayMs / 1000,
    });
    const rightAnim = animate(rightEdgeMv, plan.rightEdge, {
      type: 'spring',
      ...INSERT_TEXT_MOTION.recenterSpring,
      delay: plan.recenterDelayMs / 1000,
    });
    return () => {
      widthAnim.stop();
      rightAnim.stop();
    };
  }, [
    plan.width,
    plan.rightEdge,
    plan.widthDelayMs,
    plan.recenterDelayMs,
    reduceMotion,
    rightEdgeMv,
    widthMv,
  ]);

  const overflowing = contentWidth + padX * 2 >= maxWidth - 0.5;

  return (
    <div
      className="ol-live-transcript-stage"
      data-tone={tone}
      data-controls={hasControls ? 'true' : 'false'}
      style={{ width: '100%', height, maxWidth: stageWidth }}
    >
      <motion.div
        className="ol-live-transcript-pill"
        data-tone={tone}
        style={{
          left: leftMv,
          width: widthMv,
          height,
          pointerEvents: hasControls ? 'auto' : 'none',
        }}
      >
        {onCancel && (
          <button
            type="button"
            className="ol-live-transcript-control"
            style={{ width: controlSize, height: controlSize }}
            aria-label={cancelLabel}
            disabled={!cancelEnabled}
            onMouseDown={(event) => event.preventDefault()}
            onClick={onCancel}
          >
            <Icon name="close" size={Math.round(controlSize * 0.42)} strokeWidth={2.1} />
          </button>
        )}
        {onConfirm && (
          <button
            type="button"
            className="ol-live-transcript-control ol-live-transcript-confirm"
            style={{ width: controlSize, height: controlSize, marginLeft: 'auto' }}
            aria-label={confirmLabel}
            disabled={!confirmEnabled}
            onMouseDown={(event) => event.preventDefault()}
            onClick={onConfirm}
          >
            <Icon name="check" size={Math.round(controlSize * 0.44)} strokeWidth={2.1} />
          </button>
        )}
      </motion.div>
      <motion.div
        className="ol-live-transcript-track"
        data-overflow={overflowing ? 'true' : 'false'}
        layoutRoot
        style={{
          right: trackRightMv,
          width: Math.max(1, contentWidth),
          height,
        }}
      >
        {units.map((unit, index) => (
          <motion.span
            key={unit.key}
            className="ol-live-transcript-char"
            layout={reduceMotion || unit.born ? false : 'position'}
            initial={
              unit.born && !reduceMotion
                ? {
                    opacity: 0,
                    y: INSERT_TEXT_MOTION.bornY,
                    filter: `blur(${INSERT_TEXT_MOTION.bornBlurPx}px)`,
                  }
                : false
            }
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            transition={
              reduceMotion
                ? { duration: 0 }
                : {
                    layout: {
                      type: 'spring',
                      ...INSERT_TEXT_MOTION.shiftSpring,
                      delay: (delays[index] ?? 0) / 1000,
                    },
                    opacity: {
                      duration: INSERT_TEXT_MOTION.bornFadeMs / 1000,
                      ease: [0.16, 1, 0.3, 1],
                      delay: (delays[index] ?? 0) / 1000,
                    },
                    y: {
                      type: 'spring',
                      ...INSERT_TEXT_MOTION.charSpring,
                      delay: (delays[index] ?? 0) / 1000,
                    },
                    filter: {
                      duration: INSERT_TEXT_MOTION.bornFadeMs / 1000,
                      ease: [0.22, 0.8, 0.22, 1],
                      delay: (delays[index] ?? 0) / 1000,
                    },
                  }
            }
          >
            {unit.text === ' ' ? '\u00a0' : unit.text}
          </motion.span>
        ))}
      </motion.div>
      <span className="ol-live-transcript-sr" role="status" aria-live="polite">
        {text}
      </span>
    </div>
  );
}
