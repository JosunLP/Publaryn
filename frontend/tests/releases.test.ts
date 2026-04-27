import { describe, expect, test } from 'bun:test';

import { getReleaseActionAvailability } from '../src/utils/releases';

describe('release action availability', () => {
  test('requires finalized releases before enabling undeprecation', () => {
    expect(
      getReleaseActionAvailability(
        { status: 'published', is_yanked: false, is_deprecated: true },
        0
      ).canUndeprecate
    ).toBe(true);

    expect(
      getReleaseActionAvailability(
        { status: 'published', is_yanked: false, is_deprecated: false },
        0
      ).canUndeprecate
    ).toBe(false);

    expect(
      getReleaseActionAvailability(
        { status: 'scanning', is_yanked: false, is_deprecated: true },
        0
      ).canUndeprecate
    ).toBe(false);

    expect(
      getReleaseActionAvailability(
        { status: 'deleted', is_yanked: false, is_deprecated: true },
        0
      ).canUndeprecate
    ).toBe(false);
  });
});
