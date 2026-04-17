function y = receiver_expression_handle(objs, s)
f0 = @s.item.total;
f1 = @objs{2}.total;
f2 = @objs.duplicate()(3).total;
y = f0;
end
