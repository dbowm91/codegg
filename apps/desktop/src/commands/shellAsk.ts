// The desktop's ask.
//
// M007-I (plan 022, work package F): the ask is the daemon's, not the
// desktop's. The desktop names the run the human picked and the question
// the human typed; the daemon reads its own run bytes, redacts the
// evidence it assembles and admits the turn with it. The desktop holds no
// copy of the evidence — the run's output never crosses the bridge.
//
// The promotion identity belongs to the ask, not to the submit: a retry of
// the same ask must stage the same identity, or the daemon would treat it
// as a second, different piece of evidence.

import type { HumanShellPromotionStageView } from '../bridge-types';

/** One ask the human is composing. */
export interface ShellAsk {
  /** The daemon's run id for the shell command whose output is the evidence. */
  runId: string;
  /** The identity the daemon stages the evidence under. */
  promotionId: string;
}

/**
 * A fresh identity for one ask.
 *
 * The identity is unique per ask: the daemon treats a repeated identity as
 * the same staged evidence, so two asks must never share one.
 */
export function newShellAsk(runId: string): ShellAsk {
  const random = globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2);
  return { runId, promotionId: `desktop-ask-${random}` };
}

/**
 * The promotion one ask sends.
 *
 * The question is the human's own words; the daemon reads the run's bytes
 * and assembles the evidence around it.
 */
export function shellAskPromotion(ask: ShellAsk, question: string): HumanShellPromotionStageView {
  return { promotionId: ask.promotionId, sourceId: ask.runId, question };
}
