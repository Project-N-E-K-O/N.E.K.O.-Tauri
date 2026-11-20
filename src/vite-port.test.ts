// @vitest-environment node

import { parsePort, port } from '../vite.config';

// The Rust side is `raw.trim().parse::<u16>().ok().filter(|p| *p != 0)`.
// These cases must stay in lockstep or Vite and the desktop shell disagree.
describe('parsePort matches Rust u16 port parsing', () => {
  it('accepts plain integers, surrounding whitespace, leading + and leading zeros', () => {
    expect(parsePort('47911')).toBe(47911);
    expect(parsePort(' 47911 ')).toBe(47911);
    expect(parsePort('+47911')).toBe(47911);
    expect(parsePort('007')).toBe(7);
    expect(parsePort('65535')).toBe(65535);
  });

  it('rejects decimal, exponent, hex, negative, zero, overflow and empty input', () => {
    for (const raw of ['47911.0', '1e4', '0x1F', '-1', '0', '65536', '', '  ']) {
      expect(parsePort(raw), raw).toBeUndefined();
    }
    expect(parsePort(undefined)).toBeUndefined();
  });

  it('falls through an invalid first variable to the next one, then the default', () => {
    const previousNeko = process.env.NEKO_MAIN_SERVER_PORT;
    const previous = process.env.MAIN_SERVER_PORT;

    try {
      process.env.NEKO_MAIN_SERVER_PORT = '47911.0';
      process.env.MAIN_SERVER_PORT = '47912';
      expect(port('MAIN_SERVER_PORT', 48911)).toBe(47912);

      delete process.env.MAIN_SERVER_PORT;
      expect(port('MAIN_SERVER_PORT', 48911)).toBe(48911);
    } finally {
      if (previousNeko === undefined) delete process.env.NEKO_MAIN_SERVER_PORT;
      else process.env.NEKO_MAIN_SERVER_PORT = previousNeko;
      if (previous === undefined) delete process.env.MAIN_SERVER_PORT;
      else process.env.MAIN_SERVER_PORT = previous;
    }
  });
});
