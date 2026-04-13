x = [1 2; 3 4];
x(:, 3) = [5; 6];
x(3, :) = [7 8 9];
x(10) = 11;
xcol = x(:, end);
xlast = x(end);

c = {1, 2; 3, 4};
c(:, 3) = {5; 6};
c(3, :) = {7, 8, 9};
c{10} = 10;
ccol = c(:, end);
clast = c{10};

y = [1; 2; 3];
y(4) = 4;
ylast = y(end);

d = {1; 2; 3};
d(4) = {4};
dlast = d{4};

z = cat(3, [1 2; 3 4], [5 6; 7 8]);
z(:, 5) = [9; 10];
zcol = z(:, end);

cz = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});
cz(:, 5) = {9; 10};
czcol = cz(:, end);

lg = cat(3, [1 2; 3 4], [5 6; 7 8]);
lg(13) = 9;
lglast = lg(end);
lgsize = size(lg);

lcg = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});
lcg{13} = 90;
lcglast = lcg{end};
lcgsize = size(lcg);
