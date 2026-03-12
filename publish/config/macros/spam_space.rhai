// Rapidly repeats space bar while the bound button is held.
// Each cycle: press 15ms, release, sleep 15ms.
while held() {
    press_key("space");
    sleep(15);
    release_key("space");
    sleep(15);
}
