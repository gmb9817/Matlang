a = cat(3, [1 2; 3 4], [5 6; 7 8]);
a_last_col = a(:, end);
a_tail = a(1, 2:end);
a_head = a(2, 1:end-1);
a(:, end) = [60; 80];
a_page2 = a(:, :, 2);

cells = cat(3, {10 20; 30 40}, {50 60; 70 80});
cells_last_col = cells(:, end);
cells(:, end) = {600; 800};
cells_page2 = cells(:, :, 2);

s(1, 1, 1).x = 1;
s(1, 1, 2).x = 2;
s_value = s(end).x;
s(1, end).x = 22;
s_page2 = s(:, :, 2);

txt = 'hello';
txt_tail = txt(2:end-1);
txt(1, end-1:end) = 'XY';
txt_after = txt;
