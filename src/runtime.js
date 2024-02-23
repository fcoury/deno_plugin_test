const { core } = Deno;
const { ops } = core;

const editor = {
  add_to_buffer: async (text) => {
    return ops.op_add_to_buffer(text);
  },

  get_buffer: async () => {
    return ops.op_get_buffer();
  },
};

globalThis.editor = editor;
