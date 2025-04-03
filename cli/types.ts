export interface KeyConfig {
  command: Deno.Command;
  isShiftKey: (output: string) => boolean;
  isKeyDown: (output: string) => boolean;
  isKeyUp: (output: string) => boolean;
  isEscapeKey: (output: string) => boolean;
}

export interface Config {
  commonWords: string[];
  instructions: {
    model: string;
    temperature: number;
    tips: string[];
  };
}
