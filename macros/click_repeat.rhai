// Rapidly repeats left click while the bound button is held.
// Each cycle: click, sleep 30ms.
while held() {
    click("left");
    sleep(30);
}
