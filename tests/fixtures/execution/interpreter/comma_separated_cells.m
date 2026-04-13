cells = {10, 20, 30};
[a, b, c] = cells{:};
[d, e] = cells{2:3};
[x, y, z] = deal(cells{:});
row = [cells{:}];
copy = {cells{:}};
tableau = [cells{:}; cells{:}];

labels = {'north', 'east'; 'south', 'west'};
[g, h] = labels{:, 2};
[p, q, r, s] = deal(labels{:});
column = {labels{:, 2}};

updated = {0, 0, 0};
[updated{:}] = deal(100, 200, 300);

matrix_cells = {0, 0; 0, 0};
[matrix_cells{1, :}] = deal(1, 2);
plain_deal_cells = {0, 0};
plain_deal_cells{1:2} = deal(10, 20);
