a = reshape(1:8, [2 2 2]);
q13 = quantile(a, [0.25 0.5 0.75], [1 3]);
q123 = quantile(a, [0.25 0.5 0.75], [1 2 3]);
p13 = prctile(a, [25 50 75], [1 3]);
p123 = prctile(a, [25 50 75], [1 2 3]);
m = reshape(1:4, [2 2]);
q4 = quantile(m, [0.25 0.75], [4]);
p4 = prctile(m, [25 75], [4]);
