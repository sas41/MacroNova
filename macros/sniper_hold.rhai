// Sniper button macro: hold Shift while button is pressed.
// The held() function returns false when the button is released,
// causing this loop to exit.

press_key("shift");
while held() {
    sleep(10);
}
release_key("shift");
