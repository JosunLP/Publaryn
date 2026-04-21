declare module 'bun:test' {
  export function describe(
    name: string,
    callback: () => void | Promise<void>
  ): void;

  export function test(
    name: string,
    callback: () => void | Promise<void>
  ): void;

  export function afterEach(
    callback: () => void | Promise<void>
  ): void;

  export const mock: {
    module(
      specifier: string,
      factory: () => Record<string, unknown>
    ): void;
  };

  export function expect<T = unknown>(actual: T): any;
}
