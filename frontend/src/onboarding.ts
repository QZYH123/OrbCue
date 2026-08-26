export const ONBOARDING_STEPS = ['theme', 'connect', 'run'] as const;
export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];
export const ONBOARDING_STORAGE_KEY = 'onboarding-complete';

export function nextOnboardingStep(step: OnboardingStep): OnboardingStep | 'done' {
  return ONBOARDING_STEPS[ONBOARDING_STEPS.indexOf(step) + 1] ?? 'done';
}

export function onboardingStepIndex(step: OnboardingStep): number {
  return ONBOARDING_STEPS.indexOf(step) + 1;
}

export function initialOnboardingComplete(
  search: string,
  storage: Pick<Storage, 'getItem'> | null,
): boolean {
  if (new URLSearchParams(search).get('onboarding') === '1') return false;
  try {
    return storage?.getItem(ONBOARDING_STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

export function persistOnboardingComplete(storage: Pick<Storage, 'setItem'> | null) {
  try {
    storage?.setItem(ONBOARDING_STORAGE_KEY, 'true');
  } catch {
    /* ignore quota */
  }
}
