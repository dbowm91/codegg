import { describe, expect, it } from 'vitest';
import { newShellAsk, shellAskPromotion } from './shellAsk';

// M007-I (plan 022, work package F): the desktop names the run and the
// question. It holds no evidence, so the ask's whole content is these two
// facts plus the identity the daemon stages it under.
describe('the desktop ask', () => {
  it('names the run the human picked and the question they typed', () => {
    const ask = newShellAsk('run-1');
    expect(ask.runId).toBe('run-1');
    expect(ask.promotionId).not.toBe('');
    expect(shellAskPromotion(ask, 'why did it fail?')).toEqual({
      promotionId: ask.promotionId,
      sourceId: 'run-1',
      question: 'why did it fail?',
    });
  });

  it('gives two asks two identities, so the daemon cannot mistake one for the other', () => {
    expect(newShellAsk('run-1').promotionId).not.toBe(newShellAsk('run-1').promotionId);
  });

  it('keeps one identity across the retry of one ask', () => {
    const ask = newShellAsk('run-1');
    expect(shellAskPromotion(ask, 'why?').promotionId).toBe(
      shellAskPromotion(ask, 'why?').promotionId,
    );
  });
});
