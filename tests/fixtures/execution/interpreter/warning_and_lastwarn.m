msg0 = "unset";
id0 = "unset";
msg1 = "unset";
id1 = "unset";
msg2 = "unset";
id2 = "unset";
msg3 = "unset";
id3 = "unset";
single = "unset";

warning("plain warning");
[msg0, id0] = lastwarn();

warning("MATC:FmtWarn", "value=%d", 7);
[msg1, id1] = lastwarn();

warn_handle = @warning;
warn_handle("MATC:HandleWarn", "state=%s", "ready");

query_handle = @lastwarn;
[msg2, id2] = query_handle();
single = lastwarn();

lastwarn("", "");
[msg3, id3] = lastwarn();
