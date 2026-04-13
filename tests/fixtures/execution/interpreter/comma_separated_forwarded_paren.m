cells = {[10, 20], [30, 40]};
[a, b] = cells{:}(2);
row = [cells{:}(1)];
copies = {cells{:}(2)};

nested = {struct("score", [1, 2]), struct("score", [3, 4])};
[c, d] = nested{:}.score(2);
scores = [nested{:}.score(1)];
