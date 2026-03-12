// Rapidly repeats right click while the bound button is held.
// Each cycle: press 15ms, release, sleep 15ms.
while held() {
    press_mouse("right");
    sleep(15);
    release_mouse("right");
    sleep(15);
}
