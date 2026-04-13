a = cat(3, [1 2; 3 4], [5 6; 7 8]);
mask = cat(3, [true false; false true], [false true; true false]);
selected = a(mask);
a(mask) = [10 20 30 40];
assigned = a;

clear a mask
