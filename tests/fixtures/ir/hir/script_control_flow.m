a = 1;
b = 2;
c = 3;
d = 4;
x = [a b; c d];
mode = 1;
switch mode
case 0
    y = zeros(1, 2);
otherwise
    y = sum(x);
end
