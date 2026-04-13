state0 = pause('query');
old1 = pause('off');
state1 = pause('query');

pause(0);

old2 = pause("on");
state2 = pause("query");

pause_handle = @pause;
old3 = pause_handle('off');
state3 = pause('query');
pause(old3);
state4 = pause('query');
