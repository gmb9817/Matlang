state0 = warning("query");
warning("off");
state1 = warning("query");

warning("MATC:Silent", "suppressed=%d", 3);
[msg1, id1] = lastwarn();

warning("on", "MATC:Only");
state_id0 = warning("query", "MATC:Only");
warning("MATC:Only", "allowed=%d", 7);
[msg2, id2] = lastwarn();
state_id1 = warning("query", "MATC:Other");

warning_handle = @warning;
state2 = warning_handle("query", "all");
warning_handle("off", "all");
state3 = warning("query", "all");

warning("on", "all");
state4 = warning("query", "all");

warning("off", "MATC:Specific");
state5 = warning("query", "MATC:Specific");
warning("MATC:Specific", "blocked");
[msg3, id3] = lastwarn();
warning("on", "MATC:Specific");
warning("MATC:Specific", "live");
[msg4, id4] = lastwarn();
