// Rapidly repeats left click while the bound button is held.
// Each cycle: press 15ms, release, sleep 15ms.
while held() {
    press_mouse("left");
    sleep(15);
    release_mouse("left");
    sleep(15);
}
