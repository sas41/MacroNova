// Moves the cursor to absolute screen position (960, 540).
// Logs the position before and after for debugging.
let before = get_mouse_pos();
log(`before: ${before[0]}, ${before[1]}`);

warp_mouse(960, 540);

let after = get_mouse_pos();
log(`after: ${after[0]}, ${after[1]}`);
