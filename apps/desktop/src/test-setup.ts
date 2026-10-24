import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// The desktop suite has no global test environment, so Testing Library's
// automatic cleanup is not registered. Without it a rendered surface
// stays mounted for the rest of its file and a later assertion finds
// two elements where it expects one.
afterEach(() => {
  cleanup();
});
