a = reshape([4 1 3 2 8 5 7 6], [2 2 2]);
[sorted3, idx3] = sort(a, 3, "descend");
flag3 = issorted(a, 3, "descend");
flag3_sorted = issorted(sorted3, 3, "descend");
