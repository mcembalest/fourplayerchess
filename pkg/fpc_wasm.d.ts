/* tslint:disable */
/* eslint-disable */

export function fpc_analyze(history_json: string, level: number): string;

export function fpc_attack_map(pos_json: string, color: string): string;

export function fpc_best_move(pos_json: string, level: number): string;

export function fpc_eval(pos_json: string): string;

export function fpc_legal_moves(pos_json: string): string;

/**
 * The initial-position packet (so the UI and engine agree on the start).
 */
export function fpc_new_game(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly fpc_new_game: () => [number, number];
    readonly fpc_legal_moves: (a: number, b: number) => [number, number];
    readonly fpc_best_move: (a: number, b: number, c: number) => [number, number];
    readonly fpc_eval: (a: number, b: number) => [number, number];
    readonly fpc_analyze: (a: number, b: number, c: number) => [number, number];
    readonly fpc_attack_map: (a: number, b: number, c: number, d: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
