await editor.add_to_buffer("hey");
console.log(await editor.get_buffer());

await editor.add_to_buffer(" there");
console.log(await editor.get_buffer());
