import { describe, expect, it } from 'vitest';
import {
  initialOnboardingComplete,
  nextOnboardingStep,
  ONBOARDING_STORAGE_KEY,
  onboardingStepIndex,
  persistOnboardingComplete,
} from './onboarding';

describe('onboarding steps', () => {
  it('goes theme → connect → run → done', () => {
    expect(nextOnboardingStep('theme')).toBe('connect');
    expect(nextOnboardingStep('connect')).toBe('run');
    expect(nextOnboardingStep('run')).toBe('done');
  });

  it('numbers steps from 1', () => {
    expect(onboardingStepIndex('theme')).toBe(1);
    expect(onboardingStepIndex('connect')).toBe(2);
    expect(onboardingStepIndex('run')).toBe(3);
  });
});

describe('onboarding persistence', () => {
  it('forces the guide from ?onboarding=1 even after completion', () => {
    const storage = {
      getItem: (key: string) => (key === ONBOARDING_STORAGE_KEY ? 'true' : null),
    };
    expect(initialOnboardingComplete('?onboarding=1', storage)).toBe(false);
    expect(initialOnboardingComplete('', storage)).toBe(true);
    expect(initialOnboardingComplete('', { getItem: () => null })).toBe(false);
  });

  it('writes the completion flag', () => {
    const written: Record<string, string> = {};
    persistOnboardingComplete({
      setItem: (key, value) => {
        written[key] = value;
      },
    });
    expect(written[ONBOARDING_STORAGE_KEY]).toBe('true');
  });
});
